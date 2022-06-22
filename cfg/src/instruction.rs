pub mod location;

pub(crate) mod branch_info;
pub mod value_info;

mod for_loop;
mod phi;
mod terminator;

mod binary;
mod unary;

mod call;
mod concat;

mod closure;
mod load;
mod r#move;
mod store;

use std::fmt;

pub use phi::Phi;
pub use terminator::{ConditionalJump, Return, Terminator, UnconditionalJump};

pub use binary::{Binary, BinaryOp};
pub use unary::{Unary, UnaryOp};

pub use call::Call;
pub use concat::Concat;
pub use for_loop::{IterativeFor, NumericFor};

pub use closure::{Closure, Upvalue};
pub use load::{LoadConstant, LoadGlobal, LoadIndex, LoadTable, LoadUpvalue};
pub use r#move::Move;
pub use store::{StoreGlobal, StoreIndex, StoreUpvalue};

use super::value::ValueId;
use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use value_info::ValueInfo;

/// A struct that represents an instruction in the IR that is not a terminator or phi.
#[enum_dispatch(ValueInfo)]
#[derive(Debug, Clone, EnumAsInner)]
pub enum Inner<'cfg> {
    Binary(Binary),
    Unary(Unary),
    LoadConstant(LoadConstant<'cfg>),
    LoadGlobal(LoadGlobal<'cfg>),
    LoadUpvalue(LoadUpvalue),
    LoadIndex(LoadIndex),
    LoadTable(LoadTable),
    Move(Move),
    StoreGlobal(StoreGlobal<'cfg>),
    StoreUpvalue(StoreUpvalue),
    StoreIndex(StoreIndex),
    Concat(Concat),
    Call(Call),
    Closure(Closure<'cfg>),
}

impl fmt::Display for Inner<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::Binary(v) => write!(f, "{}", v),
            Self::Unary(v) => write!(f, "{}", v),
            Self::LoadConstant(v) => write!(f, "{}", v),
            Self::LoadGlobal(v) => write!(f, "{}", v),
            Self::LoadUpvalue(v) => write!(f, "{}", v),
            Self::LoadIndex(v) => write!(f, "{}", v),
            Self::LoadTable(v) => write!(f, "{}", v),
            Self::Move(v) => write!(f, "{}", v),
            Self::StoreGlobal(v) => write!(f, "{}", v),
            Self::StoreUpvalue(v) => write!(f, "{}", v),
            Self::StoreIndex(v) => write!(f, "{}", v),
            Self::Concat(v) => write!(f, "{}", v),
            Self::Call(v) => write!(f, "{}", v),
            Self::Closure(v) => write!(f, "{}", v),
        }
    }
}