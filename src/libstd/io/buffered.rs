// Copyright 2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Buffering wrappers for I/O traits
//!
//! It can be excessively inefficient to work directly with a `Reader` or
//! `Writer`. Every call to `read` or `write` on `TcpStream` results in a
//! system call, for example. This module provides structures that wrap
//! `Readers`, `Writers`, and `Streams` and buffer input and output to them.
//!
//! # Examples
//!
//! ```
//! let tcp_stream = TcpStream::connect(addr);
//! let reader = BufferedReader::new(tcp_stream);
//!
//! let mut buf: ~[u8] = vec::from_elem(100, 0u8);
//! match reader.read(buf.as_slice()) {
//!     Some(nread) => println!("Read {} bytes", nread),
//!     None => println!("At the end of the stream!")
//! }
//! ```
//!
//! ```
//! let tcp_stream = TcpStream::connect(addr);
//! let writer = BufferedWriter::new(tcp_stream);
//!
//! writer.write("hello, world".as_bytes());
//! writer.flush();
//! ```
//!
//! ```
//! let tcp_stream = TcpStream::connect(addr);
//! let stream = BufferedStream::new(tcp_stream);
//!
//! stream.write("hello, world".as_bytes());
//! stream.flush();
//!
//! let mut buf = vec::from_elem(100, 0u8);
//! match stream.read(buf.as_slice()) {
//!     Some(nread) => println!("Read {} bytes", nread),
//!     None => println!("At the end of the stream!")
//! }
//! ```
//!

use prelude::*;

use num;
use vec;
use super::{Stream, Decorator};

// libuv recommends 64k buffers to maximize throughput
// https://groups.google.com/forum/#!topic/libuv/oQO1HJAIDdA
static DEFAULT_CAPACITY: uint = 64 * 1024;

/// Wraps a Reader and buffers input from it
pub struct BufferedReader<R> {
    priv inner: R,
    priv buf: ~[u8],
    priv pos: uint,
    priv cap: uint
}

impl<R: Reader> BufferedReader<R> {
    /// Creates a new `BufferedReader` with with the specified buffer capacity
    pub fn with_capacity(cap: uint, inner: R) -> BufferedReader<R> {
        // It's *much* faster to create an uninitialized buffer than it is to
        // fill everything in with 0. This buffer is entirely an implementation
        // detail and is never exposed, so we're safe to not initialize
        // everything up-front. This allows creation of BufferedReader instances
        // to be very cheap (large mallocs are not nearly as expensive as large
        // callocs).
        let mut buf = vec::with_capacity(cap);
        unsafe { vec::raw::set_len(&mut buf, cap); }
        BufferedReader {
            inner: inner,
            buf: buf,
            pos: 0,
            cap: 0
        }
    }

    /// Creates a new `BufferedReader` with a default buffer capacity
    pub fn new(inner: R) -> BufferedReader<R> {
        BufferedReader::with_capacity(DEFAULT_CAPACITY, inner)
    }
}

impl<R: Reader> Buffer for BufferedReader<R> {
    fn fill<'a>(&'a mut self) -> &'a [u8] {
        if self.pos == self.cap {
            match self.inner.read(self.buf) {
                Some(cap) => {
                    self.pos = 0;
                    self.cap = cap;
                }
                None => {}
            }
        }
        return self.buf.slice(self.pos, self.cap);
    }

    fn consume(&mut self, amt: uint) {
        self.pos += amt;
        assert!(self.pos <= self.cap);
    }
}

impl<R: Reader> Reader for BufferedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Option<uint> {
        let nread = {
            let available = self.fill();
            if available.len() == 0 {
                return None;
            }
            let nread = num::min(available.len(), buf.len());
            vec::bytes::copy_memory(buf, available, nread);
            nread
        };
        self.pos += nread;
        Some(nread)
    }

    fn eof(&mut self) -> bool {
        self.pos == self.cap && self.inner.eof()
    }
}

impl<R: Reader> Decorator<R> for BufferedReader<R> {
    fn inner(self) -> R { self.inner }
    fn inner_ref<'a>(&'a self) -> &'a R { &self.inner }
    fn inner_mut_ref<'a>(&'a mut self) -> &'a mut R { &mut self.inner }
}

/// Wraps a Writer and buffers output to it
///
/// Note that `BufferedWriter` will NOT flush its buffer when dropped.
pub struct BufferedWriter<W> {
    priv inner: W,
    priv buf: ~[u8],
    priv pos: uint
}

impl<W: Writer> BufferedWriter<W> {
    /// Creates a new `BufferedWriter` with with the specified buffer capacity
    pub fn with_capacity(cap: uint, inner: W) -> BufferedWriter<W> {
        // See comments in BufferedReader for why this uses unsafe code.
        let mut buf = vec::with_capacity(cap);
        unsafe { vec::raw::set_len(&mut buf, cap); }
        BufferedWriter {
            inner: inner,
            buf: buf,
            pos: 0
        }
    }

    /// Creates a new `BufferedWriter` with a default buffer capacity
    pub fn new(inner: W) -> BufferedWriter<W> {
        BufferedWriter::with_capacity(DEFAULT_CAPACITY, inner)
    }

    fn flush_buf(&mut self) {
        if self.pos != 0 {
            self.inner.write(self.buf.slice_to(self.pos));
            self.pos = 0;
        }
    }
}

impl<W: Writer> Writer for BufferedWriter<W> {
    fn write(&mut self, buf: &[u8]) {
        if self.pos + buf.len() > self.buf.len() {
            self.flush_buf();
        }

        if buf.len() > self.buf.len() {
            self.inner.write(buf);
        } else {
            let dst = self.buf.mut_slice_from(self.pos);
            vec::bytes::copy_memory(dst, buf, buf.len());
            self.pos += buf.len();
        }
    }

    fn flush(&mut self) {
        self.flush_buf();
        self.inner.flush();
    }
}

impl<W: Writer> Decorator<W> for BufferedWriter<W> {
    fn inner(mut self) -> W { self.flush_buf(); self.inner }
    fn inner_ref<'a>(&'a self) -> &'a W { &self.inner }
    fn inner_mut_ref<'a>(&'a mut self) -> &'a mut W { &mut self.inner }
}

/// Wraps a Writer and buffers output to it, flushing whenever a newline (0xa,
/// '\n') is detected.
///
/// Note that this structure does NOT flush the output when dropped.
pub struct LineBufferedWriter<W> {
    priv inner: BufferedWriter<W>,
}

impl<W: Writer> LineBufferedWriter<W> {
    /// Creates a new `LineBufferedWriter`
    pub fn new(inner: W) -> LineBufferedWriter<W> {
        // Lines typically aren't that long, don't use a giant buffer
        LineBufferedWriter {
            inner: BufferedWriter::with_capacity(1024, inner)
        }
    }
}

impl<W: Writer> Writer for LineBufferedWriter<W> {
    fn write(&mut self, buf: &[u8]) {
        match buf.iter().rposition(|&b| b == '\n' as u8) {
            Some(i) => {
                self.inner.write(buf.slice_to(i + 1));
                self.inner.flush();
                self.inner.write(buf.slice_from(i + 1));
            }
            None => self.inner.write(buf),
        }
    }

    fn flush(&mut self) { self.inner.flush() }
}

impl<W: Writer> Decorator<W> for LineBufferedWriter<W> {
    fn inner(self) -> W { self.inner.inner() }
    fn inner_ref<'a>(&'a self) -> &'a W { self.inner.inner_ref() }
    fn inner_mut_ref<'a>(&'a mut self) -> &'a mut W { self.inner.inner_mut_ref() }
}

struct InternalBufferedWriter<W>(BufferedWriter<W>);

impl<W: Reader> Reader for InternalBufferedWriter<W> {
    fn read(&mut self, buf: &mut [u8]) -> Option<uint> { self.inner.read(buf) }
    fn eof(&mut self) -> bool { self.inner.eof() }
}

/// Wraps a Stream and buffers input and output to and from it
///
/// Note that `BufferedStream` will NOT flush its output buffer when dropped.
pub struct BufferedStream<S> {
    priv inner: BufferedReader<InternalBufferedWriter<S>>
}

impl<S: Stream> BufferedStream<S> {
    pub fn with_capacities(reader_cap: uint, writer_cap: uint, inner: S)
                           -> BufferedStream<S> {
        let writer = BufferedWriter::with_capacity(writer_cap, inner);
        let internal_writer = InternalBufferedWriter(writer);
        let reader = BufferedReader::with_capacity(reader_cap,
                                                   internal_writer);
        BufferedStream { inner: reader }
    }

    pub fn new(inner: S) -> BufferedStream<S> {
        BufferedStream::with_capacities(DEFAULT_CAPACITY, DEFAULT_CAPACITY,
                                        inner)
    }
}

impl<S: Stream> Buffer for BufferedStream<S> {
    fn fill<'a>(&'a mut self) -> &'a [u8] { self.inner.fill() }
    fn consume(&mut self, amt: uint) { self.inner.consume(amt) }
}

impl<S: Stream> Reader for BufferedStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> Option<uint> { self.inner.read(buf) }
    fn eof(&mut self) -> bool { self.inner.eof() }
}

impl<S: Stream> Writer for BufferedStream<S> {
    fn write(&mut self, buf: &[u8]) { self.inner.inner.write(buf) }
    fn flush(&mut self) { self.inner.inner.flush() }
}

impl<S: Stream> Decorator<S> for BufferedStream<S> {
    fn inner(self) -> S { self.inner.inner.inner() }
    fn inner_ref<'a>(&'a self) -> &'a S { self.inner.inner.inner_ref() }
    fn inner_mut_ref<'a>(&'a mut self) -> &'a mut S {
        self.inner.inner.inner_mut_ref()
    }
}

#[cfg(test)]
mod test {
    use prelude::*;
    use super::*;
    use io;
    use super::super::mem::{MemReader, MemWriter};
    use Harness = extra::test::BenchHarness;

    /// A type, free to create, primarily intended for benchmarking creation of wrappers that, just
    /// for construction, don't need a Reader/Writer that does anything useful. Is equivalent to
    /// `/dev/null` in semantics.
    #[deriving(Clone,Eq,Ord)]
    pub struct NullStream;

    impl Reader for NullStream {
        fn read(&mut self, _: &mut [u8]) -> Option<uint> {
            None
        }

        fn eof(&mut self) -> bool {
            true
        }
    }

    impl Writer for NullStream {
        fn write(&mut self, _: &[u8]) { }
    }

    #[test]
    fn test_buffered_reader() {
        let inner = MemReader::new(~[0, 1, 2, 3, 4]);
        let mut reader = BufferedReader::with_capacity(2, inner);

        let mut buf = [0, 0, 0];
        let nread = reader.read(buf);
        assert_eq!(Some(2), nread);
        assert_eq!([0, 1, 0], buf);
        assert!(!reader.eof());

        let mut buf = [0];
        let nread = reader.read(buf);
        assert_eq!(Some(1), nread);
        assert_eq!([2], buf);
        assert!(!reader.eof());

        let mut buf = [0, 0, 0];
        let nread = reader.read(buf);
        assert_eq!(Some(1), nread);
        assert_eq!([3, 0, 0], buf);
        assert!(!reader.eof());

        let nread = reader.read(buf);
        assert_eq!(Some(1), nread);
        assert_eq!([4, 0, 0], buf);
        assert!(reader.eof());

        assert_eq!(None, reader.read(buf));
    }

    #[test]
    fn test_buffered_writer() {
        let inner = MemWriter::new();
        let mut writer = BufferedWriter::with_capacity(2, inner);

        writer.write([0, 1]);
        assert_eq!([], writer.inner_ref().inner_ref().as_slice());

        writer.write([2]);
        assert_eq!([0, 1], writer.inner_ref().inner_ref().as_slice());

        writer.write([3]);
        assert_eq!([0, 1], writer.inner_ref().inner_ref().as_slice());

        writer.flush();
        assert_eq!([0, 1, 2, 3], writer.inner_ref().inner_ref().as_slice());

        writer.write([4]);
        writer.write([5]);
        assert_eq!([0, 1, 2, 3], writer.inner_ref().inner_ref().as_slice());

        writer.write([6]);
        assert_eq!([0, 1, 2, 3, 4, 5],
                   writer.inner_ref().inner_ref().as_slice());

        writer.write([7, 8]);
        assert_eq!([0, 1, 2, 3, 4, 5, 6],
                   writer.inner_ref().inner_ref().as_slice());

        writer.write([9, 10, 11]);
        assert_eq!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
                   writer.inner_ref().inner_ref().as_slice());

        writer.flush();
        assert_eq!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
                   writer.inner_ref().inner_ref().as_slice());
    }

    #[test]
    fn test_buffered_writer_inner_flushes() {
        let mut w = BufferedWriter::with_capacity(3, MemWriter::new());
        w.write([0, 1]);
        assert_eq!([], w.inner_ref().inner_ref().as_slice());
        let w = w.inner();
        assert_eq!([0, 1], w.inner_ref().as_slice());
    }

    // This is just here to make sure that we don't infinite loop in the
    // newtype struct autoderef weirdness
    #[test]
    fn test_buffered_stream() {
        struct S;

        impl io::Writer for S {
            fn write(&mut self, _: &[u8]) {}
        }

        impl io::Reader for S {
            fn read(&mut self, _: &mut [u8]) -> Option<uint> { None }
            fn eof(&mut self) -> bool { true }
        }

        let mut stream = BufferedStream::new(S);
        let mut buf = [];
        stream.read(buf);
        stream.eof();
        stream.write(buf);
        stream.flush();
    }

    #[test]
    fn test_read_until() {
        let inner = MemReader::new(~[0, 1, 2, 1, 0]);
        let mut reader = BufferedReader::with_capacity(2, inner);
        assert_eq!(reader.read_until(0), Some(~[0]));
        assert_eq!(reader.read_until(2), Some(~[1, 2]));
        assert_eq!(reader.read_until(1), Some(~[1]));
        assert_eq!(reader.read_until(8), Some(~[0]));
        assert_eq!(reader.read_until(9), None);
    }

    #[test]
    fn test_line_buffer() {
        let mut writer = LineBufferedWriter::new(MemWriter::new());
        writer.write([0]);
        assert_eq!(*writer.inner_ref().inner_ref(), ~[]);
        writer.write([1]);
        assert_eq!(*writer.inner_ref().inner_ref(), ~[]);
        writer.flush();
        assert_eq!(*writer.inner_ref().inner_ref(), ~[0, 1]);
        writer.write([0, '\n' as u8, 1, '\n' as u8, 2]);
        assert_eq!(*writer.inner_ref().inner_ref(),
            ~[0, 1, 0, '\n' as u8, 1, '\n' as u8]);
        writer.flush();
        assert_eq!(*writer.inner_ref().inner_ref(),
            ~[0, 1, 0, '\n' as u8, 1, '\n' as u8, 2]);
        writer.write([3, '\n' as u8]);
        assert_eq!(*writer.inner_ref().inner_ref(),
            ~[0, 1, 0, '\n' as u8, 1, '\n' as u8, 2, 3, '\n' as u8]);
    }

    #[bench]
    fn bench_buffered_reader(bh: &mut Harness) {
        bh.iter(|| {
            BufferedReader::new(NullStream);
        });
    }

    #[bench]
    fn bench_buffered_writer(bh: &mut Harness) {
        bh.iter(|| {
            BufferedWriter::new(NullStream);
        });
    }

    #[bench]
    fn bench_buffered_stream(bh: &mut Harness) {
        bh.iter(|| {
            BufferedStream::new(NullStream);
        });
    }
}
