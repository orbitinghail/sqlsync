use std::io::{self, Read, Seek, SeekFrom, Write};

/**
 * The traits in this file copy certain methods and docstrings from the position-io module
 * https://crates.io/crates/positioned-io
 * postioned-io is under the MIT license
 *
 * They are copied here as the positioned-io crate takes a lot of dependencies
 * on std::File which we don't need or want due to Wasm limitations.
 */

pub trait PositionedReader {
    /// Reads bytes from an offset in this source into a buffer, returning how
    /// many bytes were read.
    ///
    /// This function may yield fewer bytes than the size of `buf`, if it was
    /// interrupted or hit the "end of file".  ///
    /// See [`Read::read()`](https://doc.rust-lang.org/std/io/trait.Read.html#tymethod.read)
    /// for details.
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize>;

    /// Get the size of this object, in bytes.
    fn size(&self) -> io::Result<usize>;

    /// Reads the exact number of bytes required to fill `buf` from an offset.
    ///
    /// Errors if the "end of file" is encountered before filling the buffer.
    ///
    /// See [`Read::read_exact()`](https://doc.rust-lang.org/std/io/trait.Read.html#method.read_exact)
    /// for details.
    fn read_exact_at(&self, mut pos: usize, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.read_at(pos, buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                    pos += n;
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if !buf.is_empty() {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ))
        } else {
            Ok(())
        }
    }

    fn read_all(&self) -> io::Result<Vec<u8>> {
        let mut out = vec![0; self.size()?];
        self.read_exact_at(0, &mut out)?;
        Ok(out)
    }
}

pub trait PositionedWriter {
    /// Writes bytes from a buffer to an offset, returning the number of bytes
    /// written.
    ///
    /// This function may write fewer bytes than the size of `buf`, for example
    /// if it is interrupted.
    ///
    /// See [`Write::write()`](https://doc.rust-lang.org/std/io/trait.Write.html#tymethod.write)
    /// for details.
    fn write_at(&mut self, pos: usize, buf: &[u8]) -> io::Result<usize>;

    /// Flush this writer, ensuring that any intermediately buffered data
    /// reaches its destination.
    ///
    /// This should rarely do anything, since buffering is not very useful for
    /// positioned writes.
    ///
    /// This should be equivalent to
    /// [`Write::flush()`](https://doc.rust-lang.org/std/io/trait.Write.html#tymethod.flush),
    /// so it does not actually sync changes to disk when writing a `File`.
    /// Use
    /// [`File::sync_data()`](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_data)
    /// instead.
    fn flush(&mut self) -> io::Result<()>;

    /// Writes a complete buffer at an offset.
    ///
    /// Errors if it could not write the entire buffer.
    ///
    /// See [`Write::write_all()`](https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all)
    /// for details.
    fn write_all_at(&mut self, mut pos: usize, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.write_at(pos, buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "failed to write whole buffer",
                    ))
                }
                Ok(n) => {
                    buf = &buf[n..];
                    pos += n;
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl PositionedReader for Vec<u8> {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        self.as_slice().read_at(pos, buf)
    }

    fn size(&self) -> io::Result<usize> {
        Ok(self.len())
    }
}

impl<'a> PositionedReader for &'a [u8] {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        if pos >= self.len() {
            return Ok(0);
        }
        let bytes = buf.len().min(self.len() - pos);
        buf[..bytes].copy_from_slice(&self[pos..(pos + bytes)]);
        Ok(bytes)
    }

    fn size(&self) -> io::Result<usize> {
        Ok(self.len())
    }
}

pub struct PositionedCursor<I> {
    inner: I,
    pos: usize,
}

impl<I> PositionedCursor<I> {
    pub fn new(inner: I) -> Self {
        Self { inner, pos: 0 }
    }
}

impl<I: PositionedReader> Read for PositionedCursor<I> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes = self.inner.read_at(self.pos, buf)?;
        self.pos += bytes;
        Ok(bytes)
    }
}

impl<I: PositionedWriter> Write for PositionedCursor<I> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let pos = self.pos;
        let bytes = self.inner.write_at(pos, buf)?;
        self.pos += bytes;
        Ok(bytes)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<I: PositionedReader> Seek for PositionedCursor<I> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let pos = match pos {
            SeekFrom::Start(p) => p as usize,
            SeekFrom::Current(p) => self.pos + (p as usize),
            SeekFrom::End(p) => {
                let size = self.inner.size()?;
                size + (p as usize)
            }
        };
        self.pos = pos;
        Ok(self.pos as u64)
    }
}

// Ref implementations

impl<'a, T: ?Sized + PositionedReader> PositionedReader for &'a T {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        T::read_at(self, pos, buf)
    }

    fn size(&self) -> io::Result<usize> {
        T::size(self)
    }
}

impl<'a, T: ?Sized + PositionedReader> PositionedReader for &'a mut T {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        T::read_at(self, pos, buf)
    }

    fn size(&self) -> io::Result<usize> {
        T::size(self)
    }
}

impl<'a, T: ?Sized + PositionedWriter> PositionedWriter for &'a mut T {
    fn write_at(&mut self, pos: usize, buf: &[u8]) -> io::Result<usize> {
        T::write_at(self, pos, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        T::flush(self)
    }
}
