use crate::errors::RequestError;
use derive_getters::Getters;
use std::io::{Read, Write};

#[derive(Debug, Getters)]
pub(crate) struct RequestLine {
    method: String,
    path: String,
}

#[derive(Debug, Getters)]
pub(crate) struct Request {
    request_line: RequestLine,
    body: String,
}

pub(crate) fn send_response<T: Write>(
    stream: &mut T,
    mut headers: Vec<String>,
    body: Option<String>,
) {
    headers.push("Connection: close".to_string());

    let response = match body {
        Some(body) => {
            headers.push(format!("Content-Length: {}", body.len()));
            let headers_string: String = headers.join("\r\n");
            format!("{headers_string}\r\n\r\n{body}")
        }
        None => headers.join("\r\n"),
    };
    log::trace!("Sending response =\n{response}\nEOF");
    match stream.write(response.as_bytes()) {
        Ok(_) => {}
        Err(e) => log::error!("Failed to flush HTTP response. {:?}", e),
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(crate) fn get_request<T: Read>(stream: &mut T) -> Result<Request, RequestError> {
    let mut read = vec![];
    let mut buffer = vec![0; 1024];
    let mut body_start_index = None;
    let mut expected_len = None;

    loop {
        match stream.read(&mut buffer[..]) {
            Ok(0) => {
                log::trace!("EOF found");
                break;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Note: the right thing would be reading the buffer everytime,
                // looking for content-length, and seeing if we have got that many
                // bytes. If so, break. Will likely do in the future.
                log::trace!("WouldBlock- assuming end of transmission.");
                break;
            }
            Err(e) => {
                log::error!("Failed to read from stream. {:?}", e);
                return Err(RequestError::StreamRead(e));
            }
            Ok(bytes_read) => {
                log::trace!("Read {} bytes from incoming stream.", bytes_read);
                read.extend_from_slice(&buffer[0..bytes_read]);
            }
        }

        // Check if we've gotten all the headers.
        if body_start_index.is_none() {
            log::trace!("Looking for body_start_index");
            match find_subsequence(&buffer, b"\r\n\r\n") {
                Some(len) => {
                    body_start_index = Some(len + "\r\n\r\n".len());
                }
                None => {}
            }
            log::trace!("body_start_index is now {:?}", body_start_index);
        }
        // Check if we've gotten all the content
        if body_start_index.is_some() && expected_len.is_none() {
            // TODO: not case sensitive.
            log::trace!("Looking for expected_len / content_length");
            let content_length_index = find_subsequence(&buffer, b"Content-Length: ")
                .ok_or(RequestError::NoContentLength)?;
            let end_of_line = find_subsequence(&buffer[content_length_index..], b"\r")
                .ok_or(RequestError::NoContentLength)?
                + content_length_index;
            let start_index = content_length_index + "Content-Length: ".len();
            let content_length = std::str::from_utf8(&buffer[start_index..end_of_line])
                .map_err(RequestError::BadMessage)?;
            log::trace!("Parced content_length as '{content_length}'");
            let as_usize = content_length
                .parse::<usize>()
                .map_err(|_| RequestError::NoContentLength)?;
            expected_len = Some(as_usize);
            log::trace!("expected len is now {:?}", expected_len);
        }

        match expected_len {
            None => {}
            Some(len) => {
                if read.len() >= len {
                    break;
                }
            }
        }
    }

    log::trace!("Recieved full request, now seperating headers and body.");
    let end_index = find_subsequence(&read, b"\n").ok_or(RequestError::NoRequestLine)?;
    let request_line_slice = &read[0..end_index];
    let mut request_line_str = std::str::from_utf8(request_line_slice)
        .map_err(RequestError::BadMessage)?
        .split(' ');
    let request_line = RequestLine {
        method: request_line_str
            .next()
            .ok_or(RequestError::RequestLineParse)?
            .to_string(),
        path: request_line_str
            .next()
            .ok_or(RequestError::RequestLineParse)?
            .to_string(),
    };
    log::trace!("Request line = {:?}", request_line);

    let start_index =
        find_subsequence(&read, b"\r\n\r\n").ok_or(RequestError::NoMessageBody)? + "\r\n\r\n".len();
    let end_index = start_index + expected_len.unwrap();
    if end_index > read.len() {
        let actual = read.len() - start_index;
        return Err(RequestError::BadContentLength(
            expected_len.unwrap(),
            actual,
        ));
    }
    let body_slice = &read[start_index..end_index];
    let body = std::str::from_utf8(body_slice)
        .map_err(RequestError::BadMessage)?
        .to_string();
    log::trace!("Request body =\n{body}\nEOF");

    Ok(Request { request_line, body })
}

#[cfg(test)]
mod test {
    use super::*;

    struct MockWriter {
        data: Vec<u8>,
    }

    impl MockWriter {
        fn new() -> Self {
            MockWriter { data: vec![] }
        }

        fn to_string(&self) -> String {
            std::str::from_utf8(&self.data)
                .expect("Failed to convert data to string")
                .to_string()
        }
    }

    impl Write for MockWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.data.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn send_response_with_none() {
        let mut stream = MockWriter::new();
        let headers = vec![
            "X-Something: Or the other".to_string(),
            "X-Order: persists".to_string(),
        ];
        send_response(&mut stream, headers, None);
        let output = stream.to_string();
        let expected = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close";
        assert_eq!(expected, output);
    }

    #[test]
    fn send_response_with_some() {
        let mut stream = MockWriter::new();
        let headers = vec![
            "X-Something: Or the other".to_string(),
            "X-Order: persists".to_string(),
        ];
        let body = "Nala".to_string();
        send_response(&mut stream, headers, Some(body));
        let output = stream.to_string();
        let expected = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala";
        assert_eq!(expected, output);
    }

    #[test]
    fn get_request_happy_case() {
        let message = "GET / HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala";
        let expected_body = "Nala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request).expect("Failed to parse request");
        assert_eq!(result.body(), expected_body);
        assert_eq!(result.request_line().method(), "GET");
        assert_eq!(result.request_line().path(), "/");
    }

    #[test]
    fn get_request_extra_data() {
        let message = "POST /somewhere HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala is the best dog.";
        let expected = "Nala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request).expect("Failed to parse request");
        assert_eq!(result.body(), expected);
        assert_eq!(result.request_line().method(), "POST");
        assert_eq!(result.request_line().path(), "/somewhere");
    }

    #[test]
    fn get_request_missing_data() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala is the best dog.";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request);
        assert!(matches!(
            result,
            Err(RequestError::BadContentLength(42, 21))
        ));
    }

    #[test]
    fn get_request_no_content_length() {
        let message =
            "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\n\r\nNala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request);
        assert!(matches!(result, Err(RequestError::NoContentLength)));
    }

    #[test]
    fn get_request_bad_content_length() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: four\r\n\r\nNala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request);
        assert!(matches!(result, Err(RequestError::NoContentLength)));
    }

    #[test]
    fn get_request_bad_request_line_empty() {
        // Without \r\n, it would think X-Something: is the method and "or" is the path.
        let message = "\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request);
        assert!(matches!(result, Err(RequestError::RequestLineParse)));
    }

    #[test]
    fn get_request_bad_request_line_no_path() {
        let message = "GET\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_request(&mut request);
        assert!(matches!(result, Err(RequestError::RequestLineParse)));
    }
}
