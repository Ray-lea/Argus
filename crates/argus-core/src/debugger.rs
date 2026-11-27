//! Attaching the registered debugger to a process, as Task Manager's "Debug"
//! does. The debugger is the one Windows starts for a crashing program
//! (`AeDebug\Debugger`, set by Visual Studio, WinDbg's `-I` and others): its
//! command line is a printf format taking the process ID, then an event the
//! debugger signals once attached.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_ALL_ACCESS, PROCESS_INFORMATION, STARTUPINFOEXW,
    UpdateProcThreadAttribute,
};
use windows::core::{PWSTR, w};

use crate::actions::{ActionError, criticality, essential_image, image_name, open_instance};
use crate::hardware::registry::reg_string;
use crate::util::{OwnedHandle, wide};

/// The command line Windows runs to debug a crashing program, if one is set.
fn template() -> Option<String> {
    reg_string(
        w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AeDebug"),
        w!("Debugger"),
    )
    .filter(|command| !command.is_empty())
}

/// The registered debugger's file name, for the menu to name it; `None`
/// when no debugger that takes a process ID is registered, and Task Manager
/// offers no "Debug" either.
pub fn registered() -> Option<String> {
    let template = template()?;
    command_line(&template, 0, 0)?;
    program(&template)
}

/// The program `template` starts: its first word, or the quoted path.
fn program(template: &str) -> Option<String> {
    let template = template.trim_start();
    let path = match template.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => {
            // An unquoted path may hold spaces: it ends with its extension.
            let lower = template.to_ascii_lowercase();
            match lower.find(".exe") {
                Some(end) => &template[..end + 4],
                None => template.split_whitespace().next()?,
            }
        }
    };
    let name = path.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// `template` with its printf conversions filled as Windows fills them for a
/// crash: the process ID, the event handle, then 0 for any further one (the
/// address of the crash's `JIT_DEBUG_INFO`, which there is none of). `None`
/// for a template without a process ID or with a conversion it cannot take.
fn command_line(template: &str, pid: u32, event: usize) -> Option<String> {
    let mut out = String::with_capacity(template.len() + 24);
    let mut chars = template.chars().peekable();
    let mut filled = 0usize;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if chars.next_if_eq(&'%').is_some() {
            out.push('%');
            continue;
        }
        let mut zero = false;
        while let Some(flag) = chars.next_if(|c| "-+ #0".contains(*c)) {
            zero |= flag == '0';
        }
        let mut width = 0usize;
        while let Some(digit) = chars.next_if(char::is_ascii_digit) {
            width = width * 10 + digit.to_digit(10)? as usize;
        }
        // Length modifiers: l, ll, h, hh, I, I32, I64.
        while chars.next_if(|c| matches!(c, 'l' | 'h' | 'I')).is_some() {
            while chars.next_if(char::is_ascii_digit).is_some() {}
        }
        let value = match filled {
            0 => u64::from(pid),
            1 => event as u64,
            _ => 0,
        };
        filled += 1;
        let text = match chars.next()? {
            'd' | 'i' | 'u' => value.to_string(),
            'x' => format!("{value:x}"),
            'X' => format!("{value:X}"),
            'p' => format!("{value:0width$X}", width = 2 * size_of::<usize>()),
            _ => return None,
        };
        let fill = if zero { '0' } else { ' ' };
        for _ in text.len()..width {
            out.push(fill);
        }
        out.push_str(&text);
    }
    (filled > 0).then_some(out)
}

/// Starts the registered debugger on the process instance `pid` +
/// `create_time`, with the caller's rights: attaching takes full access to
/// the process, so another account's or an elevated process needs an elevated
/// caller. The debugger may stop the process, and ending the debugging
/// session usually ends the process too, so critical processes, whose end
/// stops Windows, and the images `actions::essential_image` names, whose stop
/// freezes the screen or Argus, are refused.
///
/// The debugger runs as Argus does, elevated or not, as Task Manager's does:
/// a debugger with fewer rights than the process cannot attach to it.
pub fn attach(pid: u32, create_time: i64) -> Result<(), ActionError> {
    if pid == 0 || pid == 4 {
        return Err(ActionError::Refused);
    }
    if create_time <= 0 {
        return Err(ActionError::NotFound);
    }
    // Held until the debugger runs, so the ID cannot pass to another process.
    let target = open_instance(pid, create_time, PROCESS_ALL_ACCESS)?;
    if criticality(&target)? {
        return Err(ActionError::Critical);
    }
    if essential_image(&image_name(target.0)?) {
        return Err(ActionError::Refused);
    }
    let template =
        template().ok_or_else(|| ActionError::Os(crate::tr!("core.debugger.unregistered")))?;
    // Signalled by the debugger once attached; nothing waits for it here, and
    // the debugger keeps its own copy once this one is closed.
    let inherit = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let event = OwnedHandle(unsafe { CreateEventW(Some(&inherit), true, false, None) }?);
    let command = command_line(&template, pid, event.0.0 as usize)
        .ok_or_else(|| ActionError::Os(crate::tr!("core.debugger.unregistered")))?;
    launch(&command, event.0).inspect_err(|error| {
        crate::diagnostics::windows_failure("debug process CreateProcessW", error)
    })?;
    drop(target);
    Ok(())
}

/// Runs `command`, passing it `event` and no other handle of Argus's.
fn launch(command: &str, event: HANDLE) -> windows::core::Result<()> {
    let mut bytes = 0;
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut bytes) };
    let mut list = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(list.as_mut_ptr().cast());
    unsafe { InitializeProcThreadAttributeList(Some(pointer), 1, None, &mut bytes) }?;
    struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
    impl Drop for Attributes {
        fn drop(&mut self) {
            unsafe { DeleteProcThreadAttributeList(self.0) };
        }
    }
    let attributes = Attributes(pointer);
    let inherited = [event];
    unsafe {
        UpdateProcThreadAttribute(
            attributes.0,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(inherited.as_ptr().cast()),
            size_of_val(&inherited),
            None,
            None,
        )
    }?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.0;
    let mut command = wide(command);
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            None,
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT,
            None,
            None,
            &startup.StartupInfo,
            &mut info,
        )
    }?;
    drop(OwnedHandle(info.hThread));
    drop(OwnedHandle(info.hProcess));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{command_line, launch, program};
    use crate::util::OwnedHandle;
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    const VISUAL_STUDIO: &str = r#""C:\WINDOWS\system32\vsjitdebugger.exe" -p %ld -e %ld -j 0x%p"#;
    const WINDBG: &str =
        r#""C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\windbg.exe" -p %ld -e %ld -g"#;

    #[test]
    fn the_process_id_and_event_fill_the_registered_command_in_order() {
        assert_eq!(
            command_line(VISUAL_STUDIO, 1234, 0x2c8).unwrap(),
            r#""C:\WINDOWS\system32\vsjitdebugger.exe" -p 1234 -e 712 -j 0x0000000000000000"#
        );
        assert_eq!(
            command_line(WINDBG, 42, 16).unwrap(),
            r#""C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\windbg.exe" -p 42 -e 16 -g"#
        );
        assert_eq!(
            command_line("dbg.exe %u %08lx 100%%", 7, 255).unwrap(),
            "dbg.exe 7 000000ff 100%"
        );
        assert_eq!(
            command_line("drwtsn32 -p %I64d", 9, 0).unwrap(),
            "drwtsn32 -p 9"
        );
    }

    #[test]
    fn a_command_without_a_process_id_or_with_another_conversion_is_not_a_debugger() {
        assert_eq!(command_line(r#""C:\tools\dbg.exe" -attach"#, 1, 2), None);
        assert_eq!(command_line("dbg.exe -p %ld -name %s", 1, 2), None);
        assert_eq!(command_line("dbg.exe -p %", 1, 2), None);
    }

    #[test]
    fn the_menu_names_the_debuggers_program() {
        assert_eq!(program(VISUAL_STUDIO).as_deref(), Some("vsjitdebugger.exe"));
        assert_eq!(program(WINDBG).as_deref(), Some("windbg.exe"));
        assert_eq!(
            program(r"C:\Program Files\Debug Tools\cdb.exe -p %ld -e %ld").as_deref(),
            Some("cdb.exe")
        );
        assert_eq!(program("ntsd -p %ld").as_deref(), Some("ntsd"));
        assert_eq!(program(r#""" -p %ld"#), None);
    }

    /// The debugger receives the event in its handle table under the value
    /// its command line names: a stand-in signals it through that value.
    #[test]
    fn the_debugger_inherits_the_event_its_command_line_names() {
        let inherit = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: true.into(),
        };
        let event =
            OwnedHandle(unsafe { CreateEventW(Some(&inherit), true, false, None) }.unwrap());
        let exe = std::env::current_exe().unwrap();
        let template = format!(
            r#""{}" --exact debugger::tests::stand_in_debugger --ignored --quiet -- %ld %ld"#,
            exe.display()
        );
        let command = command_line(&template, std::process::id(), event.0.0 as usize).unwrap();
        launch(&command, event.0).unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(event.0, 30_000) },
            WAIT_OBJECT_0
        );
    }

    /// Run by `the_debugger_inherits_the_event_its_command_line_names`: sets
    /// the event whose value follows the process ID on its command line.
    #[test]
    #[ignore = "started by another test"]
    fn stand_in_debugger() {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Threading::SetEvent;
        let args: Vec<String> = std::env::args().collect();
        let at = args.iter().position(|arg| arg == "--").unwrap();
        let event: usize = args[at + 2].parse().unwrap();
        unsafe { SetEvent(HANDLE(event as *mut _)) }.unwrap();
    }
}
