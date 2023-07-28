use std::io;

use crate::positioned_io::PositionedReader;

pub trait Serializable {
    /// serialize the object into the given writer
    fn serialize_into<W: io::Write>(&self, writer: &mut W) -> io::Result<()>;
}

pub trait Deserializable: Sized {
    /// deserialize the object from the given reader
    fn deserialize_from<R: PositionedReader>(reader: R) -> io::Result<Self>;
}
