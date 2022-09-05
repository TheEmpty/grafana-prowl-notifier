use crate::errors::RequestError;
use std::io::{Read, Write};

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

pub(crate) fn get_body<T: Read>(stream: &mut T) -> Result<String, RequestError> {
    let mut read = vec![];
    let mut buffer = vec![0; 1024];
    let mut start_index = None;
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
        if start_index.is_none() {
            log::trace!("Looking for start_index");
            match find_subsequence(&buffer, b"\r\n\r\n") {
                Some(len) => {
                    start_index = Some(len + "\r\n\r\n".len());
                }
                None => {}
            }
            log::trace!("start_index is now {:?}", start_index);
        }
        // Check if we've gotten all the content
        if start_index.is_some() && expected_len.is_none() {
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
    let slice = &read[start_index..end_index];
    let request = std::str::from_utf8(slice).map_err(RequestError::BadMessage)?;
    log::trace!("Request body =\n{request}\nEOF");
    Ok(request.to_string())
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
    fn get_body_happy_case() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala";
        let expected = "Nala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_body(&mut request).expect("Failed to parse request");
        assert_eq!(result, expected);
    }

    #[test]
    fn get_body_extra_data() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala is the best dog.";
        let expected = "Nala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_body(&mut request).expect("Failed to parse request");
        assert_eq!(result, expected);
    }

    #[test]
    fn get_body_missing_data() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala is the best dog.";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_body(&mut request);
        assert!(matches!(
            result,
            Err(RequestError::BadContentLength(42, 21))
        ));
    }

    #[test]
    fn get_body_no_content_length() {
        let message =
            "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\n\r\nNala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_body(&mut request);
        assert!(matches!(result, Err(RequestError::NoContentLength)));
    }

    #[test]
    fn get_body_bad_content_length() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: four\r\n\r\nNala";
        let mut request = std::io::BufReader::new(message.as_bytes());
        let result = get_body(&mut request);
        assert!(matches!(result, Err(RequestError::NoContentLength)));
    }
}
