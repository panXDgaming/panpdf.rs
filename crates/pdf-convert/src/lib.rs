pub mod catalogue;

#[cfg(all(feature = "run", not(target_arch = "wasm32")))]
pub mod run;

pub use catalogue::{
    Area, Choice, Fault, Group, Inputs, Kind, Made, Setting, SettingKind, Stroke, Tool, Value,
    Values, Why,
};
