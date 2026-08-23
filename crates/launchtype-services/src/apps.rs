//! Finding and starting the applications installed on this machine — the I/O
//! half of `@` mode (the pure rules live in [`launchtype_core::apps`]).
//!
//! Windows asks the shell for the Applications folder, the virtual folder that
//! `shell:AppsFolder` opens and the Start Menu searches: it already merges Start
//! Menu shortcuts with Store apps, so there is no second list to reconcile and
//! nothing to walk. macOS asks Spotlight for every application bundle it has
//! indexed, and falls back to walking the applications folders on a machine
//! where indexing is off.
//!
//! Neither list is filtered. `@` is meant to reach whatever the Start Menu or
//! Launchpad reaches — help files and control panels included — and silently
//! dropping a row is worse than a row nobody presses Enter on.

use launchtype_core::apps::{normalize, App, AppTarget};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LaunchError(pub String);

/// Every application this machine can start, sorted by name.
///
/// Never fails: a shell that refuses to enumerate leaves an empty list, which
/// the mode can show, rather than an error the mode would have to explain.
pub fn scan_apps() -> Vec<App> {
    normalize(platform_scan())
}

/// Start an app. On Windows every target goes back through the shell that
/// listed it; on macOS through `open`, the only supported way to start a bundle.
#[cfg(windows)]
pub fn launch(target: &AppTarget) -> Result<(), LaunchError> {
    match target {
        // `explorer.exe shell:AppsFolder\<AUMID>` is the documented way to
        // activate an Applications-folder entry without COM: it works for
        // packaged apps, for Start Menu shortcuts and for the synthesised
        // control-panel entries alike, and — because Explorer is the one doing
        // the starting — the app comes up unelevated even when Launchtype is
        // running as administrator.
        AppTarget::AppUserModelId(id) => std::process::Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{id}"))
            .spawn()
            .map(|_| ())
            .map_err(|e| LaunchError(e.to_string())),
        AppTarget::Path(path) => open::that_detached(path).map_err(|e| LaunchError(e.to_string())),
    }
}

#[cfg(not(windows))]
pub fn launch(target: &AppTarget) -> Result<(), LaunchError> {
    // A .app is a directory: spawning it fails with EACCES, so it goes through
    // `open`, which finds the executable inside and hands the app to the window
    // server so it comes up focused (same reasoning as `runner::open_app_bundle`).
    let path = target.as_str();
    match std::process::Command::new("/usr/bin/open").arg(path).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(LaunchError(format!("open failed for {path} ({status})"))),
        Err(e) => Err(LaunchError(e.to_string())),
    }
}

/// The program file behind an app, when the OS knows of one.
///
/// Resolved on demand rather than during the scan: reading the property for
/// every one of a few hundred apps adds ~70ms to entering the mode, and the
/// answer is wanted for the one row somebody asked about, which costs a
/// millisecond or two.
///
/// A packaged Windows app has no answer, and that is not a failure to work
/// around. Windows starts it by identity; its files sit in an ACL'd
/// `WindowsApps` folder under a version-stamped name that changes with every
/// update, so there is no path worth handing anyone.
#[cfg(windows)]
pub fn executable_path(target: &AppTarget) -> Option<String> {
    match target {
        AppTarget::AppUserModelId(id) => windows_apps::link_target(id),
        AppTarget::Path(path) => Some(path.clone()),
    }
}

/// On macOS the bundle *is* the application — it is what Finder shows, what
/// `open` takes and what goes in a command's path. The Mach-O binary inside
/// `Contents/MacOS` is an implementation detail nothing else refers to.
#[cfg(not(windows))]
pub fn executable_path(target: &AppTarget) -> Option<String> {
    Some(target.as_str().to_string())
}

#[cfg(windows)]
fn platform_scan() -> Vec<App> {
    windows_apps::apps_folder()
}

/// Windows: enumerate the shell's Applications folder.
///
/// The folder is virtual, so there is nothing to read off disk — it is bound to
/// an enumerator through COM, and every item is asked for two names: the one to
/// show, and its parse name, which for this folder is the Application User Model
/// ID that starts it again later.
#[cfg(windows)]
mod windows_apps {
    use launchtype_core::apps::{App, AppTarget};
    use windows::core::{GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, IEnumShellItems, IShellItem, IShellItem2, SHCreateItemFromParsingName,
        SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    };

    /// The Applications folder, by CLSID. The friendlier `shell:AppsFolder`
    /// alias means the same folder but reaches it through a registry lookup;
    /// the CLSID is the folder itself and needs nothing looked up.
    const APPS_FOLDER: &str = "shell:::{4234D49B-0245-4DF3-B780-3893943456E1}";

    /// How many items to pull from the enumerator at a time. The shell fills the
    /// whole array in one call, so this only decides how many round trips the
    /// walk costs; a few hundred apps is the normal size of the folder.
    const BATCH: usize = 64;

    /// `System.Link.TargetParsingPath` — the file an Applications-folder entry
    /// stands for. Spelled out rather than looked up by name through
    /// `PSGetPropertyKeyFromName`, which is a call into propsys for a value
    /// that has not changed since Vista; `the_link_property_key_is_right`
    /// checks the two against each other so a typo cannot survive.
    pub const PKEY_LINK_TARGET_PARSING_PATH: PROPERTYKEY =
        PROPERTYKEY { fmtid: GUID::from_u128(0xb9b4b3fc_2b51_4a42_b5d8_324146afcf25), pid: 2 };

    /// COM initialised for this thread *by us*, and so ours to undo.
    ///
    /// The scan runs on whichever thread asked for it, and the UI thread already
    /// has COM up (wxWidgets initialises it). Re-initialising there comes back
    /// `S_FALSE` — still ours to balance — or `RPC_E_CHANGED_MODE`, which is not:
    /// uninitialising an apartment we did not open would take the clipboard and
    /// the file dialogs down with it.
    struct ComGuard(bool);

    impl ComGuard {
        fn new() -> ComGuard {
            // SAFETY: callable on any thread at any time; the returned HRESULT
            // says whether this call is the one that has to undo it.
            let hr: HRESULT = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            ComGuard(hr.is_ok())
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances the successful CoInitializeEx above, on the
                // same thread (the guard never leaves the function that made it).
                unsafe { CoUninitialize() };
            }
        }
    }

    /// Read one of an item's names, copying it out of the shell's allocator.
    fn name_of(item: &IShellItem, kind: SIGDN) -> Option<String> {
        // SAFETY: `item` is a live interface pointer. GetDisplayName either
        // fails or yields a NUL-terminated buffer that is ours to free, which is
        // what CoTaskMemFree does once the text has been copied into a String.
        unsafe {
            let raw = item.GetDisplayName(kind).ok()?;
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.as_ptr() as *const _));
            text.filter(|text| !text.is_empty())
        }
    }

    /// A NUL-terminated wide copy of `text`, for the PCWSTR arguments below.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The file one Applications-folder entry stands for, found by reopening
    /// the entry by its identity and asking for the one property.
    ///
    /// This is the general answer, not a parse of the identity string: a Start
    /// Menu program's AUMID happens to look like `{KnownFolderId}\App.exe`, but
    /// Firefox's is the opaque `308046B0AF4A39CB` and Task Manager's is a
    /// synthesised `Microsoft.AutoGenerated.{...}` — and the shell resolves a
    /// real path for all three.
    pub fn link_target(aumid: &str) -> Option<String> {
        let _com = ComGuard::new();
        let path = wide(&format!("shell:AppsFolder\\{aumid}"));
        // SAFETY: `path` outlives the call that reads it; the item, if it comes
        // back at all, is a live interface pointer, and the string it yields is
        // ours to free once copied.
        let target = unsafe {
            let item: IShellItem2 =
                SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()?;
            let raw = item.GetString(&PKEY_LINK_TARGET_PARSING_PATH).ok()?;
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.as_ptr() as *const _));
            text.filter(|text| !text.is_empty())
        }?;

        // The property is a *parsing* path, so it is whatever the entry points
        // at — and a Start Menu shortcut is free to point at a URL rather than
        // a file. A Steam game's is `steam://rungameid/N`, which answers a
        // different question than the one being asked here.
        (!launchtype_core::portable::looks_like_url(&target)).then_some(target)
    }

    pub fn apps_folder() -> Vec<App> {
        let _com = ComGuard::new();
        let mut apps = Vec::new();
        let path = wide(APPS_FOLDER);

        // SAFETY: `path` outlives the call that reads it, and every interface is
        // checked before use. A shell that will not produce the folder or the
        // enumerator leaves the list empty rather than failing the mode.
        unsafe {
            let folder: IShellItem = match SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None)
            {
                Ok(folder) => folder,
                Err(e) => {
                    log::warn!("cannot open the applications folder: {e}");
                    return apps;
                }
            };
            let items: IEnumShellItems = match folder.BindToHandler(None, &BHID_EnumItems) {
                Ok(items) => items,
                Err(e) => {
                    log::warn!("cannot enumerate the applications folder: {e}");
                    return apps;
                }
            };

            loop {
                let mut batch: [Option<IShellItem>; BATCH] = std::array::from_fn(|_| None);
                let mut fetched = 0u32;
                // The end of the folder is reported as S_FALSE with nothing
                // fetched, and S_FALSE is a success code: what ends the loop is
                // the count, not the Result.
                if items.Next(&mut batch, Some(&mut fetched)).is_err() {
                    break;
                }
                if fetched == 0 {
                    break;
                }
                for item in batch.iter().take(fetched as usize).flatten() {
                    let (Some(name), Some(id)) = (
                        name_of(item, SIGDN_NORMALDISPLAY),
                        name_of(item, SIGDN_PARENTRELATIVEPARSING),
                    ) else {
                        continue;
                    };
                    apps.push(App { name, target: AppTarget::AppUserModelId(id) });
                }
            }
        }
        apps
    }
}

/// macOS: every application bundle Spotlight knows about, plus a walk of the
/// applications folders for machines with indexing turned off.
#[cfg(not(windows))]
fn platform_scan() -> Vec<App> {
    // Spotlight comes first so its paths win any tie in `normalize`; the folder
    // walk only has to add what indexing missed.
    spotlight_bundles()
        .into_iter()
        .chain(applications_folders())
        .filter_map(|path| {
            launchtype_core::apps::display_name(&path)
                .map(|name| App { name, target: AppTarget::Path(path) })
        })
        .collect()
}

/// Ask the Spotlight index for application bundles. `mdfind` is the supported
/// command-line face of the index Spotlight itself searches, so this is
/// "whatever the machine considers installed" without opening a private
/// database.
#[cfg(not(windows))]
fn spotlight_bundles() -> Vec<String> {
    let output = std::process::Command::new("/usr/bin/mdfind")
        .arg("kMDItemContentType == 'com.apple.application-bundle'")
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| line.ends_with(".app"))
            .map(str::to_string)
            .collect(),
        Ok(output) => {
            log::warn!("mdfind failed ({}); falling back to the folder walk", output.status);
            Vec::new()
        }
        Err(e) => {
            log::warn!("mdfind unavailable ({e}); falling back to the folder walk");
            Vec::new()
        }
    }
}

/// The folders a Mac keeps applications in, plus one level of the grouping
/// subfolders inside them (`/Applications/Utilities`, and the per-vendor folders
/// that installers like Adobe and Microsoft create) that hold bundles of their
/// own. Deeper than that is a bundle's own insides, not another app.
#[cfg(not(windows))]
fn applications_folders() -> Vec<String> {
    let mut roots = vec![
        std::path::PathBuf::from("/Applications"),
        std::path::PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }

    let mut found = Vec::new();
    for root in roots {
        for subfolder in bundles_in(&root, &mut found) {
            bundles_in(&subfolder, &mut found);
        }
    }
    found
}

/// Append every `.app` directly inside `dir` to `out`, and return the plain
/// subfolders for the caller to descend into if it wants to.
#[cfg(not(windows))]
fn bundles_in(dir: &std::path::Path, out: &mut Vec<String>) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut subfolders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) {
            out.push(path.to_string_lossy().into_owned());
        } else if path.is_dir() {
            subfolders.push(path);
        }
    }
    subfolders
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real machine must produce a real list: the whole mode is this call.
    /// Kept loose on purpose — the names differ per machine — but a Windows or
    /// macOS box with a handful of apps means the scan broke, not that the
    /// machine is empty.
    #[test]
    fn this_machine_has_applications() {
        let apps = scan_apps();
        assert!(apps.len() > 5, "only {} apps found", apps.len());
        for app in &apps {
            assert!(!app.name.is_empty());
            assert_eq!(app.name, app.name.to_lowercase(), "{} is not lowercased", app.name);
            assert!(!app.target.as_str().is_empty(), "{} has no target", app.name);
        }
    }

    /// Sorted, deduplicated and stable enough that two scans in a row agree —
    /// the list is read out one row at a time, so an order that shuffles between
    /// scans is a bug.
    #[test]
    fn scanning_twice_gives_the_same_sorted_list() {
        let apps = scan_apps();
        assert_eq!(apps, scan_apps());

        // Accent-insensitive, so a localised Start Menu reads in order.
        let folded: Vec<String> =
            apps.iter().map(|app| launchtype_core::i18n::fold(&app.name)).collect();
        let mut sorted = folded.clone();
        sorted.sort();
        assert_eq!(folded, sorted);

        let unique: std::collections::HashSet<&AppTarget> =
            apps.iter().map(|app| &app.target).collect();
        assert_eq!(unique.len(), apps.len(), "the same app is listed twice");
    }

    /// The scan runs on the UI thread, where wxWidgets has already opened a COM
    /// apartment. Re-initialising there is refused (`RPC_E_CHANGED_MODE`) when
    /// the modes disagree, and the guard must take that for "not mine to undo"
    /// and carry on — an early return, or a `CoUninitialize` we did not earn,
    /// would leave the mode empty or tear the app's own COM down with it.
    #[cfg(windows)]
    #[test]
    fn scanning_works_inside_somebody_else_s_apartment() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        // SAFETY: this test thread owns no apartment yet; the process keeps it
        // for the rest of the run, which is what makes the point.
        let owned = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        assert!(owned.is_ok(), "could not set up the apartment this test needs");

        assert!(scan_apps().len() > 5, "the scan came back empty inside an MTA");
    }

    /// The property key is spelled out in the source for speed; propsys is the
    /// authority, and a typo in a 128-bit literal is otherwise invisible — it
    /// would just make every app look like it had no program file.
    #[cfg(windows)]
    #[test]
    fn the_link_property_key_is_right() {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::PROPERTYKEY;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;

        let name: Vec<u16> =
            "System.Link.TargetParsingPath".encode_utf16().chain(std::iter::once(0)).collect();
        let mut resolved = PROPERTYKEY::default();
        // SAFETY: propsys needs an apartment; the name outlives the call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            PSGetPropertyKeyFromName(PCWSTR(name.as_ptr()), &mut resolved).unwrap();
        }
        let ours = windows_apps::PKEY_LINK_TARGET_PARSING_PATH;
        assert_eq!(resolved.fmtid, ours.fmtid);
        assert_eq!(resolved.pid, ours.pid);
    }

    /// A path that comes back must be a file that is really there — the whole
    /// point is that it can be pasted somewhere and used.
    ///
    /// Sampled rather than exhaustive: a few hundred lookups is seconds of test
    /// time for no more confidence. Packaged apps legitimately have no answer
    /// (Windows starts them by identity), so the bar is that most of a sample
    /// resolves, not all of it.
    #[test]
    fn program_files_that_resolve_are_really_there() {
        // The same list `@` shows: Steam's Start Menu shortcuts point at a
        // steam:// URL, and this is a question about program files.
        let apps = launchtype_core::apps::without_steam(scan_apps());
        let sample: Vec<&App> = apps.iter().step_by(apps.len().div_ceil(30).max(1)).collect();
        assert!(sample.len() > 5, "sample too small to prove anything");

        let mut resolved = 0;
        for app in &sample {
            let Some(path) = executable_path(&app.target) else { continue };
            resolved += 1;
            assert!(
                std::path::Path::new(&path).exists(),
                "{:?} points at a missing {path}",
                app.name
            );
        }
        assert!(
            resolved * 2 > sample.len(),
            "only {resolved} of {} sampled apps have a program file",
            sample.len()
        );
    }

    /// An identity the shell has never heard of is a `None`, not a panic and
    /// not a plausible-looking path.
    #[cfg(windows)]
    #[test]
    fn an_unknown_app_has_no_program_file() {
        let nowhere =
            AppTarget::AppUserModelId("Launchtype.NoSuchApp_0000000000000!App".to_string());
        assert_eq!(executable_path(&nowhere), None);
    }

    /// Run by hand — it starts a real program:
    /// `cargo test -p launchtype-services -- --ignored --nocapture launches`
    #[test]
    #[ignore]
    fn launches_the_first_app() {
        let apps = scan_apps();
        let app = apps.first().expect("at least one app");
        eprintln!("launching {:?} ({})", app.name, app.target.as_str());
        assert!(launch(&app.target).is_ok());
    }
}
