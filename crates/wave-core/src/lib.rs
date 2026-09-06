//! The part of Wave Uninstaller that decides *what* to do.
//!
//! This crate never calls a Windows API. It reads the system through
//! [`provider::SystemProvider`], which `wave-win32` implements for real and
//! [`fixture::FixtureProvider`] implements from a JSON file. Keeping that seam
//! is what allows the logic that decides which files to delete to be tested
//! without a virtual machine — and given what this program does, untestable
//! decision logic is not an option.

pub mod error;
pub mod fixture;
pub mod inventory;
pub mod model;
pub mod provider;
pub mod text;

pub use error::{Error, Result};
pub use inventory::{collect, Inventory};
pub use model::{
    Architecture, InstallScope, Program, ProgramId, ProgramKind, ProgramSource, UninstallCommand,
};
pub use provider::SystemProvider;
