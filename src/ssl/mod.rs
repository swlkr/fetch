mod ffi;

use ffi::*;
use std::ffi::{CString, c_void};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::net::TcpStream;
use std::os::unix::io::AsRawFd;

pub struct SslStream {
    ssl: *mut SSL,
    ctx: *mut SSL_CTX,
    _tcp: TcpStream,
}

impl SslStream {
    pub fn connect(host: &str, port: &str) -> Result<Self> {
        unsafe {
            init();

            let method = TLS_method();
            let ctx = SSL_CTX_new(method);
            if ctx.is_null() {
                return Err(Error::new(ErrorKind::Other, "Failed to create SSL_CTX"));
            }

            let addr = format!("{}:{}", host, port);
            let tcp = TcpStream::connect(addr)?;

            let ssl = SSL_new(ctx);
            if ssl.is_null() {
                SSL_CTX_free(ctx);
                return Err(Error::new(ErrorKind::Other, "Failed to create SSL"));
            }

            SSL_set_fd(ssl, tcp.as_raw_fd());

            // Set SNI
            let c_host = CString::new(host)
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid host"))?;
            const SSL_CTRL_SET_TLSEXT_HOSTNAME: i32 = 55;
            const TLSEXT_NAMETYPE_HOST_NAME: i64 = 0;
            SSL_ctrl(
                ssl,
                SSL_CTRL_SET_TLSEXT_HOSTNAME,
                TLSEXT_NAMETYPE_HOST_NAME,
                c_host.as_ptr() as *mut c_void,
            );

            let res = SSL_connect(ssl);
            if res <= 0 {
                let err = SSL_get_error(ssl, res);
                SSL_free(ssl);
                SSL_CTX_free(ctx);
                return Err(Error::new(
                    ErrorKind::ConnectionRefused,
                    format!("SSL connect error: {}", err),
                ));
            }

            Ok(Self {
                ssl,
                ctx,
                _tcp: tcp,
            })
        }
    }

    fn get_openssl_error(&self, ret: i32) -> Error {
        let err_code = unsafe { SSL_get_error(self.ssl, ret) };
        Error::new(ErrorKind::Other, format!("OpenSSL error: {}", err_code))
    }
}

impl Write for SslStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let res = unsafe { SSL_write(self.ssl, buf.as_ptr() as *const c_void, buf.len() as i32) };
        if res > 0 {
            Ok(res as usize)
        } else {
            Err(self.get_openssl_error(res))
        }
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Read for SslStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let res = unsafe { SSL_read(self.ssl, buf.as_mut_ptr() as *mut c_void, buf.len() as i32) };
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(self.get_openssl_error(res))
        }
    }
}

impl Drop for SslStream {
    fn drop(&mut self) {
        unsafe {
            SSL_shutdown(self.ssl);
            SSL_free(self.ssl);
            SSL_CTX_free(self.ctx);
        }
    }
}
