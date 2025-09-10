use super::{function::Function, list::parse_list, parse_string};
use nom::character::complete::char;
use nom::multi::many_till;
use nom::number::complete::le_u8;
use nom::IResult;
use nom_leb128::leb128_usize;

#[derive(Debug)]
pub struct Chunk {
    pub string_table: Vec<Vec<u8>>,
    pub functions: Vec<Function>,
    pub main: usize,
}

impl Chunk {
    pub(crate) fn parse(input: &[u8], encode_key: u8, version: u8) -> IResult<&[u8], Self> {
        let (input, types_version) = if version >= 4 {
            le_u8(input)?
        } else {
            (input, 0)
        };
        if types_version > 3 {
            panic!("unsupported types version");
        }
        let (mut input, string_table) = parse_list(input, parse_string)?;
        
        if (types_version == 3) {
            //many_till(leb128_usize, char('\0'))(input)?.0

            //let userdataTypeLimit: u8 = (64 + 32) - 64;
            let (new_input, mut index) = le_u8(input)?;
            input = new_input;

            while (index != 0)
            {
                // string
                let (new_input, _) = leb128_usize(input)?;
                input = new_input;

                let (new_input, next_index) = le_u8(input)?;
                input = new_input;
                index = next_index;
            }
        };

        let (input, functions) = parse_list(input, |i| Function::parse(i, encode_key, types_version))?;
        let (input, main) = leb128_usize(input)?;

        Ok((
            input,
            Self {
                string_table,
                functions,
                main,
            },
        ))
    }
}
