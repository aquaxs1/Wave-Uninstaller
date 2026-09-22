//! The part of Wave Uninstaller that decides *what* to do.
//!
//! This crate never calls a Windows API. It reads the system through
//! [`provider::SystemProvider`] and changes it through
//! [`mutator::SystemMutator`], both of which `wave-win32` implements for real
//! and [`fixture`] implements from a JSON file. Keeping that seam is what
//! allows the logic that decides which files to delete to be tested without a
//! virtual machine — and given what this program does, untestable decision
//! logic is not an option.
//!
//! A removal runs through four steps, each of which is its own module and its
//! own set of tests:
//!
//! 1. [`inventory`] — what is installed, from every source Windows offers.
//! 2. [`scan`] — what belongs to one of those programs, filtered by
//!    [`protect`] before it is weighed by [`score`].
//! 3. [`plan`] — exactly what will happen, for a user to approve.
//! 4. [`apply`] — do it, backing up and journalling every step first, so
//!    [`undo`] can put it all back.

pub mod apply;
pub mod command;
pub mod error;
pub mod fixture;
pub mod inventory;
pub mod journal;
pub mod model;
pub mod mutator;
pub mod path;
pub mod plan;
pub mod protect;
pub mod provider;
pub mod regfile;
pub mod scan;
pub mod score;
pub mod text;
pub mod time;
pub mod undo;

pub use apply::{apply, ApplyReport};
pub use error::{Error, Result};
pub use inventory::{collect, Inventory};
pub use model::{
    Architecture, InstallScope, Program, ProgramId, ProgramKind, ProgramSource, UninstallCommand,
};
pub use mutator::SystemMutator;
pub use plan::{RemovalPlan, Selection};
pub use protect::ProtectionList;
pub use provider::SystemProvider;
pub use scan::{scan, Finding, Scan};
pub use undo::{undo, Operation};
