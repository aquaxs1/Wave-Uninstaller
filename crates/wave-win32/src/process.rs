//! Starting an uninstaller and waiting for all of it.
//!
//! The waiting is the hard part, and it is where home-grown uninstallers go
//! wrong: NSIS copies itself to `%TEMP%` and restarts from there, Inno Setup
//! does the same with `_iu*.tmp`, MSI hands the work to a service. Waiting on
//! the process that was started means the scan begins while the uninstaller is
//! still deleting things — and then reports files as leftovers that disappear
//! on their own two seconds later.
//!
//! So the process goes into a job object before it is allowed to run, and the
//! wait is for the job to empty out, not for the first process to exit.

use std::time::{Duration, Instant};

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, ResumeThread, WaitForSingleObject, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
};

use wave_core::command;
use wave_core::error::{Error, Result};
use wave_core::mutator::ProcessOutcome;

/// How often the job is asked whether anything is still running.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// A handle that closes itself.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Run a program and wait for its whole process tree.
pub fn run(program: &str, arguments: &[String], timeout_seconds: u64) -> Result<ProcessOutcome> {
    let subject = program.to_string();
    let job = unsafe { CreateJobObjectW(None, None) }
        .map_err(|error| Error::system(format!("a job object for {subject}"), error))?;
    let job = OwnedHandle(job);

    // `CreateProcessW` writes into the command line buffer, so it cannot be a
    // borrowed constant.
    let mut command_line: Vec<u16> = command::render_for_windows(program, arguments)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let startup = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut information = PROCESS_INFORMATION::default();

    // Suspended, so that the process is in the job before it can start a
    // single child of its own.
    unsafe {
        CreateProcessW(
            &HSTRING::from(program),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            None,
            None,
            &startup,
            &mut information,
        )
    }
    .map_err(|error| Error::system(subject.clone(), error))?;

    let process = OwnedHandle(information.hProcess);
    let thread = OwnedHandle(information.hThread);

    unsafe {
        AssignProcessToJobObject(job.0, process.0)
            .map_err(|error| Error::system(format!("adding {subject} to a job object"), error))?;
        ResumeThread(thread.0);
    }

    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let exit_code = wait_for_process(&process, deadline);
    let timed_out = !wait_for_job(&job, deadline) || exit_code.is_none();

    Ok(ProcessOutcome { exit_code, timed_out })
}

/// Wait for the process that was started, and read its exit code.
fn wait_for_process(process: &OwnedHandle, deadline: Instant) -> Option<i32> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let status = unsafe {
        WaitForSingleObject(process.0, remaining.as_millis().min(u128::from(u32::MAX)) as u32)
    };
    if status != WAIT_OBJECT_0 {
        return None;
    }

    let mut code = 0u32;
    unsafe { GetExitCodeProcess(process.0, &mut code) }.ok()?;
    Some(code as i32)
}

/// Wait until nothing in the job is running any more.
///
/// Polled rather than waited on through an IO completion port: the loop is a
/// dozen lines that anyone can check, the resolution it costs is a quarter of
/// a second, and an uninstaller takes seconds at best.
fn wait_for_job(job: &OwnedHandle, deadline: Instant) -> bool {
    loop {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let queried = unsafe {
            QueryInformationJobObject(
                Some(job.0),
                JobObjectBasicAccountingInformation,
                std::ptr::from_mut(&mut accounting).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        };

        // A job that cannot be queried is not a reason to wait forever.
        if queried.is_err() || accounting.ActiveProcesses == 0 {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}
