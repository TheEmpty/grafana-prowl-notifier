use std::io::{BufReader, Error, Read, Write};

pub(crate) struct TestStream<'a> {
    to_send: BufReader<&'a [u8]>,
    sent: Vec<u8>,
}

impl<'a> TestStream<'a> {
    pub(crate) fn new(to_send_data: &'a [u8]) -> Self {
        TestStream {
            to_send: BufReader::new(to_send_data),
            sent: vec![],
        }
    }
}

impl<'a> Read for TestStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.to_send.read(buf)
    }
}

impl<'a> Write for TestStream<'a> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.sent.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
