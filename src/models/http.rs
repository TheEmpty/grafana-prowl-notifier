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

#[derive(Debug, Getters)]
pub(crate) struct Response {
    status_line: String,
    headers: Vec<String>,
    body: Option<String>,
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

impl Response {
    pub(crate) fn new(status_line: String, headers: Vec<String>, body: Option<String>) -> Self {
        Response {
            status_line,
            headers,
            body,
        }
    }

    pub(crate) fn send<T: Write>(mut self, stream: &mut T) -> Result<(), std::io::Error> {
        self.headers.push("Connection: close".to_string());
        let status_line = self.status_line;

        let response = match self.body {
            Some(body) => {
                self.headers.push(format!("Content-Length: {}", body.len()));
                let headers_string: String = self.headers.join("\r\n");
                format!("{status_line}\r\n{headers_string}\r\n\r\n{body}")
            }
            None => {
                let headers_string: String = self.headers.join("\r\n");
                format!("{status_line}\r\n{headers_string}")
            }
        };
        log::trace!("Sending response =\n{response}\nEOF");
        let _ = stream.write(response.as_bytes())?;
        Ok(())
    }
}

impl Request {
    // TODO: make it not a giant blob of code
    pub(crate) fn from_stream<T: Read + Write>(stream: &mut T) -> Result<Request, RequestError> {
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
                    if find_subsequence(&read, b"Expect: 100-continue").is_some() {
                        log::trace!("Returning 100-coninue.");
                        let response = "HTTP/1.1 100 Continue\r\n".as_bytes();
                        let _ = stream.write(response).map_err(RequestError::StreamWrite)?;
                    } else {
                        log::trace!(
                            "WouldBlock without 100-continue, assuming end of transmission."
                        );
                        break;
                    }
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
                if let Some(len) = find_subsequence(&buffer, b"\r\n\r\n") {
                    body_start_index = Some(len + "\r\n\r\n".len());
                }
                log::trace!("body_start_index is now {:?}", body_start_index);
            }

            // Check if we've gotten all the content
            if body_start_index.is_some() && expected_len.is_none() {
                expected_len = try_to_get_expected_len(&buffer)?;
            }

            if let Some(len) = expected_len {
                if read.len() >= len {
                    break;
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

        let start_index = find_subsequence(&read, b"\r\n\r\n")
            .ok_or(RequestError::NoMessageBody)?
            + "\r\n\r\n".len();

        match expected_len {
            None => {
                // TODO: body as option
                if request_line.method() == "GET" {
                    Ok(Request {
                        request_line,
                        body: "".to_string(),
                    })
                } else {
                    Err(RequestError::NoContentLength)
                }
            }
            Some(expected_len) => {
                let end_index = start_index + expected_len;
                if end_index > read.len() {
                    let actual = read.len() - start_index;
                    return Err(RequestError::BadContentLength(expected_len, actual));
                }
                let body_slice = &read[start_index..end_index];
                let body = std::str::from_utf8(body_slice)
                    .map_err(RequestError::BadMessage)?
                    .to_string();
                log::trace!("Request body =\n{body}\nEOF");

                Ok(Request { request_line, body })
            }
        }
    }
}

fn try_to_get_expected_len(buffer: &[u8]) -> Result<Option<usize>, RequestError> {
    log::trace!("Looking for expected_len / content_length");
    // TODO: make this not case sensitive
    let content_length_index = match find_subsequence(buffer, b"Content-Length: ") {
        Some(v) => v,
        None => return Ok(None),
    };

    let end_of_line = find_subsequence(&buffer[content_length_index..], b"\r")
        .ok_or(RequestError::NoContentLength)?
        + content_length_index;
    let start_index = content_length_index + "Content-Length: ".len();
    let content_length =
        std::str::from_utf8(&buffer[start_index..end_of_line]).map_err(RequestError::BadMessage)?;

    log::trace!("Parced content_length as '{content_length}'");
    let as_usize = content_length
        .parse::<usize>()
        .map_err(|_| RequestError::NoContentLength)?;
    Ok(Some(as_usize))
}

// TODO: test for 100-continue
#[cfg(test)]
mod test {
    use super::*;
    use crate::test::TestStream;

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
        let status_line = "HTTP/1.1 200 OK".to_string();
        let headers = vec![
            "X-Something: Or the other".to_string(),
            "X-Order: persists".to_string(),
        ];

        let response = Response::new(status_line, headers, None);
        response
            .send(&mut stream)
            .expect("Failed to send to stream");
        let output = stream.to_string();
        let expected = "HTTP/1.1 200 OK\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close";
        assert_eq!(expected, output);
    }

    #[test]
    fn send_response_with_some() {
        let mut stream = MockWriter::new();
        let status_line = "HTTP/1.1 404 Not Found".to_string();
        let headers = vec![
            "X-Something: Or the other".to_string(),
            "X-Order: persists".to_string(),
        ];
        let body = "Nala".to_string();
        let response = Response::new(status_line, headers, Some(body));
        response
            .send(&mut stream)
            .expect("Failed to send to stream");
        let output = stream.to_string();
        let expected = "HTTP/1.1 404 Not Found\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala";
        assert_eq!(expected, output);
    }

    #[test]
    fn request_from_stream_happy_case() {
        let message = "GET / HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala";
        let expected_body = "Nala";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request).expect("Failed to parse request");
        assert_eq!(result.body(), expected_body);
        assert_eq!(result.request_line().method(), "GET");
        assert_eq!(result.request_line().path(), "/");
    }

    #[test]
    fn request_from_stream_extra_data() {
        let message = "POST /somewhere HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 4\r\n\r\nNala is the best dog.";
        let expected = "Nala";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request).expect("Failed to parse request");
        assert_eq!(result.body(), expected);
        assert_eq!(result.request_line().method(), "POST");
        assert_eq!(result.request_line().path(), "/somewhere");
    }

    #[test]
    fn request_from_stream_missing_data() {
        let message = "POST /somewhere HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala is the best dog.";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request);
        assert!(matches!(
            result,
            Err(RequestError::BadContentLength(42, 21))
        ));
    }

    #[test]
    fn request_from_stream_post_no_content_length() {
        let message =
            "POST /somewhere HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\n\r\nNala";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request);
        assert!(matches!(result, Err(RequestError::NoContentLength)));
    }

    #[test]
    fn request_from_stream_get_no_content_length() {
        let message =
            "GET /somewhere HTTP/1.1\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\n\r\n";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request).expect("Failed to parse request");
        assert_eq!(result.body(), "");
        assert_eq!(result.request_line().method(), "GET");
        assert_eq!(result.request_line().path(), "/somewhere");
    }

    #[test]
    fn request_from_stream_bad_content_length() {
        let message = "X-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: four\r\n\r\nNala";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request);
        assert!(matches!(result, Err(RequestError::NoContentLength)));
    }

    #[test]
    fn request_from_stream_bad_request_line_empty() {
        // Without \r\n, it would think X-Something: is the method and "or" is the path.
        let message = "\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request);
        assert!(matches!(result, Err(RequestError::RequestLineParse)));
    }

    #[test]
    fn request_from_stream_bad_request_line_no_path() {
        let message = "GET\r\nX-Something: Or the other\r\nX-Order: persists\r\nConnection: close\r\nContent-Length: 42\r\n\r\nNala";
        let mut request = TestStream::new(message.as_bytes());
        let result = Request::from_stream(&mut request);
        assert!(matches!(result, Err(RequestError::RequestLineParse)));
    }
}
