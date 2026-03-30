#![allow(non_camel_case_types)]

use std::ffi::{c_int, c_long, c_void};

#[repr(C)]
pub struct SSL_CTX {
    _private: [u8; 0],
}

#[repr(C)]
pub struct SSL {
    _private: [u8; 0],
}

#[repr(C)]
pub struct SSL_METHOD {
    _private: [u8; 0],
}

#[link(name = "ssl")]
#[link(name = "crypto")]
unsafe extern "C" {
    pub fn TLS_method() -> *const SSL_METHOD;
    pub fn SSL_CTX_new(method: *const SSL_METHOD) -> *mut SSL_CTX;
    pub fn SSL_CTX_free(ctx: *mut SSL_CTX);

    pub fn SSL_new(ctx: *mut SSL_CTX) -> *mut SSL;
    pub fn SSL_set_fd(ssl: *mut SSL, fd: c_int) -> c_int;
    pub fn SSL_connect(ssl: *mut SSL) -> c_int;
    pub fn SSL_free(ssl: *mut SSL);

    pub fn SSL_read(ssl: *mut SSL, buf: *mut c_void, num: c_int) -> c_int;
    pub fn SSL_write(ssl: *mut SSL, buf: *const c_void, num: c_int) -> c_int;
    pub fn SSL_shutdown(ssl: *mut SSL) -> c_int;

    pub fn SSL_get_error(ssl: *const SSL, ret: c_int) -> c_int;

    pub fn SSL_ctrl(ssl: *mut SSL, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
}

pub unsafe fn init() {
    unsafe {
        #[link(name = "crypto")]
        unsafe extern "C" {
            fn OPENSSL_init_ssl(opts: u64, settings: *const c_void) -> c_int;
        }
        OPENSSL_init_ssl(0, std::ptr::null());
    }
}
