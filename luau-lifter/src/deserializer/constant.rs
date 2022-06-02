use nom::number::complete::{le_f32, le_u32, le_u8};
use nom::IResult;
use nom_leb128::leb128_usize;

const CONSTANT_NIL: u8 = 0;
const CONSTANT_BOOLEAN: u8 = 1;
const CONSTANT_NUMBER: u8 = 2;
const CONSTANT_STRING: u8 = 3;
const CONSTANT_IMPORT: u8 = 4;
const CONSTANT_TABLE: u8 = 5;
const CONSTANT_CLOSURE: u8 = 6;

#[derive(Debug)]
pub enum Constant {
    Nil,
    Boolean(bool),
    Number(f32),
    String(usize),
    Import(usize),
    Table(Vec<usize>),
    Closure(usize),
}

impl Constant {
    pub(crate) fn parse(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, tag) = le_u8(input)?;
        match tag {
            CONSTANT_NIL => Ok((input, Constant::Nil)),
            CONSTANT_BOOLEAN => {
                let (input, value) = le_u8(input)?;
                Ok((input, Constant::Boolean(value != 0u8)))
            }
            CONSTANT_NUMBER => {
                let (input, value) = le_f32(input)?;
                Ok((input, Constant::Number(value)))
            }
            CONSTANT_STRING => {
                let (input, string_index) = leb128_usize(input)?;
                Ok((input, Constant::String(string_index)))
            }
            CONSTANT_IMPORT => {
                let (input, import_index) = le_u32(input)?;
                Ok((input, Constant::Import(import_index as usize)))
            }
            CONSTANT_TABLE | CONSTANT_CLOSURE => unimplemented!(),
            _ => panic!(),
        }
    }
}
