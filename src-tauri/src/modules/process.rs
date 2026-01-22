use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::System;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Get normalized path of the current running executable
fn get_current_exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
}

/// Check if Antigravity is running
pub fn is_antigravity_running() -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    // Recognition ref 1: Load manual config path (moved outside loop for performance)
    let manual_path = crate::modules::config::load_app_config()
        .ok()
        .and_then(|c| c.antigravity_executable)
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Exclude own path (handles case where manager is mistaken for Antigravity on Linux)
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        // Recognition ref 2: Priority check for manual path match
        if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                // macOS: Check if within the same .app bundle
                #[cfg(target_os = "macos")]
                {
                    let m_path_str = m_path.to_string_lossy();
                    let p_path_str = p_path.to_string_lossy();
                    if let (Some(m_idx), Some(p_idx)) =
                        (m_path_str.find(".app"), p_path_str.find(".app"))
                    {
                        if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                            // Even if path matches, must confirm via name and args that it's not a Helper
                            let args = process.cmd();
                            let is_helper_by_args = args
                                .iter()
                                .any(|arg| arg.to_string_lossy().contains("--type="));
                            let is_helper_by_name = name.contains("helper")
                                || name.contains("plugin")
                                || name.contains("renderer")
                                || name.contains("gpu")
                                || name.contains("crashpad")
                                || name.contains("utility")
                                || name.contains("audio")
                                || name.contains("sandbox");
                            if !is_helper_by_args && !is_helper_by_name {
                                return true;
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                if m_path == &p_path {
                    return true;
                }
            }
        }

        // Common helper process exclusion logic
        // Common helper process exclusion logic
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || name.contains("helper")
            || name.contains("plugin")
            || name.contains("renderer")
            || name.contains("gpu")
            || name.contains("crashpad")
            || name.contains("utility")
            || name.contains("audio")
            || name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            if exe_path.contains("antigravity.app") && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "windows")]
        {
            if name == "antigravity.exe" && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if (name.contains("antigravity") || exe_path.contains("/antigravity"))
                && !name.contains("tools")
                && !is_helper
            {
                return true;
            }
        }
    }

    false
}

#[cfg(target_os = "linux")]
/// Get PID set of current process and all direct relatives (ancestors + descendants)
fn get_self_family_pids(system: &sysinfo::System) -> std::collections::HashSet<u32> {
    let current_pid = std::process::id();
    let mut family_pids = std::collections::HashSet::new();
    family_pids.insert(current_pid);

    // 1. Look up all ancestors (Ancestors) - prevent killing the launcher
    let mut next_pid = current_pid;
    // Prevent infinite loop, max depth 10
    for _ in 0..10 {
        let pid_val = sysinfo::Pid::from_u32(next_pid);
        if let Some(process) = system.process(pid_val) {
            if let Some(parent) = process.parent() {
                let parent_id = parent.as_u32();
                // Avoid cycles or duplicates
                if !family_pids.insert(parent_id) {
                    break;
                }
                next_pid = parent_id;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // 2. Look down all descendants (Descendants)
    // Build parent-child relationship map (Parent -> Children)
    let mut adj: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            adj.entry(parent.as_u32()).or_default().push(pid.as_u32());
        }
    }

    // BFS traversal to find all descendants
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(current_pid);

    while let Some(pid) = queue.pop_front() {
        if let Some(children) = adj.get(&pid) {
            for &child in children {
                if family_pids.insert(child) {
                    queue.push_back(child);
                }
            }
        }
    }

    family_pids
}

/// Get PIDs of all Antigravity processes (including main and helper processes)
fn get_antigravity_pids() -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    // Linux: Enable family process tree exclusion
    #[cfg(target_os = "linux")]
    let family_pids = get_self_family_pids(&system);

    let mut pids = Vec::new();
    let current_pid = std::process::id();
    let current_exe = get_current_exe_path();

    // Load manual config path as auxiliary reference
    let manual_path = crate::modules::config::load_app_config()
        .ok()
        .and_then(|c| c.antigravity_executable)
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();

        // Exclude own PID
        if pid_u32 == current_pid {
            continue;
        }

        // Exclude own executable path (hardened against broad name matching)
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let _name = process.name().to_string_lossy().to_lowercase();

        #[cfg(target_os = "linux")]
        {
            // 1. Exclude family processes (self, children, parents)
            if family_pids.contains(&pid_u32) {
                continue;
            }
            // 2. Extra protection: match "tools" likely manager if not a child
            if _name.contains("tools") {
                continue;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Other platforms: exclude only self
            if pid_u32 == current_pid {
                continue;
            }
        }

        // Recognition ref 3: Check manual config path match
        if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                #[cfg(target_os = "macos")]
                {
                    let m_path_str = m_path.to_string_lossy();
                    let p_path_str = p_path.to_string_lossy();
                    if let (Some(m_idx), Some(p_idx)) =
                        (m_path_str.find(".app"), p_path_str.find(".app"))
                    {
                        if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                            let args = process.cmd();
                            let is_helper_by_args = args
                                .iter()
                                .any(|arg| arg.to_string_lossy().contains("--type="));
                            let is_helper_by_name = _name.contains("helper")
                                || _name.contains("plugin")
                                || _name.contains("renderer")
                                || _name.contains("gpu")
                                || _name.contains("crashpad")
                                || _name.contains("utility")
                                || _name.contains("audio")
                                || _name.contains("sandbox");
                            if !is_helper_by_args && !is_helper_by_name {
                                pids.push(pid_u32);
                                continue;
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                if m_path == &p_path {
                    pids.push(pid_u32);
                    continue;
                }
            }
        }

        // Get executable path
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Common helper process exclusion logic
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || _name.contains("helper")
            || _name.contains("plugin")
            || _name.contains("renderer")
            || _name.contains("gpu")
            || _name.contains("crashpad")
            || _name.contains("utility")
            || _name.contains("audio")
            || _name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            // Match processes within Antigravity main app bundle, excluding Helper/Plugin/Renderer etc.
            if exe_path.contains("antigravity.app") && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "windows")]
        {
            let name = process.name().to_string_lossy().to_lowercase();
            if name == "antigravity.exe" && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "linux")]
        {
            let name = process.name().to_string_lossy().to_lowercase();
            if (name == "antigravity" || exe_path.contains("/antigravity"))
                && !name.contains("tools")
                && !is_helper
            {
                pids.push(pid_u32);
            }
        }
    }

    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "Found {} Antigravity processes: {:?}",
            pids.len(),
            pids
        ));
    }

    pids
}

/// Close Antigravity processes
pub fn close_antigravity(#[allow(unused_variables)] timeout_secs: u64) -> Result<(), String> {
    crate::modules::logger::log_info("Closing Antigravity...");

    #[cfg(target_os = "windows")]
    {
        // Windows: Precise kill by PID to support multiple versions or custom filenames
        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            crate::modules::logger::log_info(&format!(
                "Precisely closing {} identified processes on Windows...",
                pids.len()
            ));
            for pid in pids {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();
            }
            // Give some time for system to clean up PIDs
            thread::sleep(Duration::from_millis(200));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: Optimize closing strategy to avoid "Window terminated unexpectedly" popups
        // Strategy: SEND SIGTERM to main process only, let it coordinate closing children

        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            // 1. Identify main process (PID)
            // Strategy: Principal processes of Electron/Tauri do not have the `--type` parameter, while Helper processes have `--type=renderer/gpu/utility`, etc.
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All);

            let mut main_pid = None;

            // Load manual configuration path as highest priority reference
            let manual_path = crate::modules::config::load_app_config()
                .ok()
                .and_then(|c| c.antigravity_executable)
                .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

            crate::modules::logger::log_info("Analyzing process list to identify main process:");
            for pid_u32 in &pids {
                let pid = sysinfo::Pid::from_u32(*pid_u32);
                if let Some(process) = system.process(pid) {
                    let name = process.name().to_string_lossy();
                    let args = process.cmd();
                    let args_str = args
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(" ");

                    crate::modules::logger::log_info(&format!(
                        " - PID: {} | Name: {} | Args: {}",
                        pid_u32, name, args_str
                    ));

                    // 1. Priority to manual path matching
                    if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                        if let Ok(p_path) = p_exe.canonicalize() {
                            let m_path_str = m_path.to_string_lossy();
                            let p_path_str = p_path.to_string_lossy();
                            if let (Some(m_idx), Some(p_idx)) =
                                (m_path_str.find(".app"), p_path_str.find(".app"))
                            {
                                if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                                    // Deep validation: even if path matches, must exclude Helper keywords and arguments
                                    let is_helper_by_args = args_str.contains("--type=");
                                    let is_helper_by_name = name.to_lowercase().contains("helper")
                                        || name.to_lowercase().contains("plugin")
                                        || name.to_lowercase().contains("renderer")
                                        || name.to_lowercase().contains("gpu")
                                        || name.to_lowercase().contains("crashpad")
                                        || name.to_lowercase().contains("utility")
                                        || name.to_lowercase().contains("audio")
                                        || name.to_lowercase().contains("sandbox")
                                        || name.to_lowercase().contains("language_server");

                                    if !is_helper_by_args && !is_helper_by_name {
                                        main_pid = Some(pid_u32);
                                        crate::modules::logger::log_info(&format!(
                                            "   => Identified as main process (manual path match)"
                                        ));
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // 2. Feature analysis matching (fallback)
                    let is_helper_by_name = name.to_lowercase().contains("helper")
                        || name.to_lowercase().contains("crashpad")
                        || name.to_lowercase().contains("utility")
                        || name.to_lowercase().contains("audio")
                        || name.to_lowercase().contains("sandbox")
                        || name.to_lowercase().contains("language_server")
                        || name.to_lowercase().contains("plugin")
                        || name.to_lowercase().contains("renderer");

                    let is_helper_by_args = args_str.contains("--type=");

                    if !is_helper_by_name && !is_helper_by_args {
                        if main_pid.is_none() {
                            main_pid = Some(pid_u32);
                            crate::modules::logger::log_info(&format!(
                                "   => Identified as main process (Name/Args analysis)"
                            ));
                        }
                    } else {
                        crate::modules::logger::log_info(&format!(
                            "   => Identified as helper process (Helper/Args)"
                        ));
                    }
                }
            }

            // Phase 1: Graceful exit (SIGTERM)
            if let Some(pid) = main_pid {
                crate::modules::logger::log_info(&format!(
                    "Sending SIGTERM to main process PID: {}",
                    pid
                ));
                let output = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();

                if let Ok(result) = output {
                    if !result.status.success() {
                        let error = String::from_utf8_lossy(&result.stderr);
                        crate::modules::logger::log_warn(&format!(
                            "Main process SIGTERM failed: {}",
                            error
                        ));
                    }
                }
            } else {
                crate::modules::logger::log_warn(
                    "No clear main process identified, attempting SIGTERM for all processes (may cause popups)",
                );
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-15", &pid.to_string()])
                        .output();
                }
            }

            // Wait for graceful exit (max 70% of timeout_secs)
            let graceful_timeout = (timeout_secs * 7) / 10;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(graceful_timeout) {
                if !is_antigravity_running() {
                    crate::modules::logger::log_info("All Antigravity processes gracefully closed");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }

            // Phase 2: Force kill (SIGKILL) - targeting all remaining processes (Helpers)
            if is_antigravity_running() {
                let remaining_pids = get_antigravity_pids();
                if !remaining_pids.is_empty() {
                    crate::modules::logger::log_warn(&format!(
                        "Graceful exit timeout, force killing {} remaining processes (SIGKILL)",
                        remaining_pids.len()
                    ));
                    for pid in &remaining_pids {
                        let output = Command::new("kill").args(["-9", &pid.to_string()]).output();

                        if let Ok(result) = output {
                            if !result.status.success() {
                                let error = String::from_utf8_lossy(&result.stderr);
                                if !error.contains("No such process") {
                                    // "No matching processes" for killall, "No such process" for kill
                                    crate::modules::logger::log_error(&format!(
                                        "SIGKILL process {} failed: {}",
                                        pid, error
                                    ));
                                }
                            }
                        }
                    }
                    thread::sleep(Duration::from_secs(1));
                }

                // Final check
                if !is_antigravity_running() {
                    crate::modules::logger::log_info("All processes exited after forced cleanup");
                    return Ok(());
                }
            } else {
                crate::modules::logger::log_info("All processes exited after SIGTERM");
                return Ok(());
            }
        } else {
            // Only consider not running when pids is empty, don't error here as it might already be closed
            crate::modules::logger::log_info("Antigravity not running, no need to close");
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: Also attempt to identify main process and delegate exit
        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All);

            let mut main_pid = None;

            // Load manual configuration path as highest priority reference
            let manual_path = crate::modules::config::load_app_config()
                .ok()
                .and_then(|c| c.antigravity_executable)
                .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

            crate::modules::logger::log_info(
                "Analyzing Linux process list to identify main process:",
            );
            for pid_u32 in &pids {
                let pid = sysinfo::Pid::from_u32(*pid_u32);
                if let Some(process) = system.process(pid) {
                    let name = process.name().to_string_lossy().to_lowercase();
                    let args = process.cmd();
                    let args_str = args
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(" ");

                    crate::modules::logger::log_info(&format!(
                        " - PID: {} | Name: {} | Args: {}",
                        pid_u32, name, args_str
                    ));

                    // 1. Priority to manual path matching
                    if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                        if let Ok(p_path) = p_exe.canonicalize() {
                            if &p_path == m_path {
                                // Confirm not a Helper
                                let is_helper_by_args = args_str.contains("--type=");
                                let is_helper_by_name = name.contains("helper")
                                    || name.contains("renderer")
                                    || name.contains("gpu")
                                    || name.contains("crashpad")
                                    || name.contains("utility")
                                    || name.contains("audio")
                                    || name.contains("sandbox");
                                if !is_helper_by_args && !is_helper_by_name {
                                    main_pid = Some(pid_u32);
                                    crate::modules::logger::log_info(&format!(
                                        "   => Identified as main process (manual path match)"
                                    ));
                                    break;
                                }
                            }
                        }
                    }

                    // 2. Feature analysis matching
                    let is_helper_by_args = args_str.contains("--type=");
                    let is_helper_by_name = name.contains("helper")
                        || name.contains("renderer")
                        || name.contains("gpu")
                        || name.contains("crashpad")
                        || name.contains("utility")
                        || name.contains("audio")
                        || name.contains("sandbox")
                        || name.contains("plugin")
                        || name.contains("language_server");

                    if !is_helper_by_args && !is_helper_by_name {
                        if main_pid.is_none() {
                            main_pid = Some(pid_u32);
                            crate::modules::logger::log_info(&format!(
                                "   => Identified as main process (Feature analysis)"
                            ));
                        }
                    } else {
                        crate::modules::logger::log_info(&format!(
                            "   => Identified as helper process (Helper/Args)"
                        ));
                    }
                }
            }

            // Phase 1: Graceful exit (SIGTERM)
            if let Some(pid) = main_pid {
                crate::modules::logger::log_info(&format!(
                    "Attempting to gracefully close main process {} (SIGTERM)",
                    pid
                ));
                let _ = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();
            } else {
                crate::modules::logger::log_warn(
                    "No clear Linux main process identified, sending SIGTERM to all associated processes",
                );
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-15", &pid.to_string()])
                        .output();
                }
            }

            // Wait for graceful exit
            let graceful_timeout = (timeout_secs * 7) / 10;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(graceful_timeout) {
                if !is_antigravity_running() {
                    crate::modules::logger::log_info("Antigravity gracefully closed");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }

            // Phase 2: Force kill (SIGKILL) - targeting all remaining processes
            if is_antigravity_running() {
                let remaining_pids = get_antigravity_pids();
                if !remaining_pids.is_empty() {
                    crate::modules::logger::log_warn(&format!(
                        "Graceful exit timeout, force killing {} remaining processes (SIGKILL)",
                        remaining_pids.len()
                    ));
                    for pid in &remaining_pids {
                        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
        } else {
            // pids is empty, meaning no process detected or all excluded by logic
            crate::modules::logger::log_info(
                "No Antigravity processes found to close (possibly filtered or not running)",
            );
        }
    }

    // Final check
    if is_antigravity_running() {
        return Err(
            "Unable to close Antigravity process, please close manually and retry".to_string(),
        );
    }

    crate::modules::logger::log_info("Antigravity closed successfully");
    Ok(())
}

/// Start Antigravity
#[allow(unused_mut)]
pub fn start_antigravity() -> Result<(), String> {
    crate::modules::logger::log_info("Starting Antigravity...");

    // Prefer manually specified path and args from configuration
    let config = crate::modules::config::load_app_config().ok();
    let manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_executable.clone());
    let args = config.and_then(|c| c.antigravity_args.clone());

    if let Some(mut path_str) = manual_path {
        let mut path = std::path::PathBuf::from(&path_str);

        #[cfg(target_os = "macos")]
        {
            // Fault tolerance: If path is inside .app bundle (e.g. misselected Helper), auto-correct to .app directory
            if let Some(app_idx) = path_str.find(".app") {
                let corrected_app = &path_str[..app_idx + 4];
                if corrected_app != path_str {
                    crate::modules::logger::log_info(&format!(
                        "Detected macOS path inside .app bundle, auto-correcting to: {}",
                        corrected_app
                    ));
                    path_str = corrected_app.to_string();
                    path = std::path::PathBuf::from(&path_str);
                }
            }
        }

        if path.exists() {
            crate::modules::logger::log_info(&format!(
                "Starting with manual configuration path: {}",
                path_str
            ));

            #[cfg(target_os = "macos")]
            {
                // macOS: if .app directory, use open
                if path_str.ends_with(".app") || path.is_dir() {
                    let mut cmd = Command::new("open");
                    cmd.arg("-a").arg(&path_str);

                    // Add startup arguments
                    if let Some(ref args) = args {
                        for arg in args {
                            cmd.arg(arg);
                        }
                    }

                    cmd.spawn()
                        .map_err(|e| format!("Startup failed (open): {}", e))?;
                } else {
                    let mut cmd = Command::new(&path_str);

                    // Add startup arguments
                    if let Some(ref args) = args {
                        for arg in args {
                            cmd.arg(arg);
                        }
                    }

                    cmd.spawn()
                        .map_err(|e| format!("Startup failed (direct): {}", e))?;
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                let mut cmd = Command::new(&path_str);

                // Add startup arguments
                if let Some(ref args) = args {
                    for arg in args {
                        cmd.arg(arg);
                    }
                }

                cmd.spawn().map_err(|e| format!("Startup failed: {}", e))?;
            }

            crate::modules::logger::log_info(&format!(
                "Antigravity startup command sent (manual path: {}, args: {:?})",
                path_str, args
            ));
            return Ok(());
        } else {
            crate::modules::logger::log_warn(&format!(
                "Manual configuration path does not exist: {}, falling back to auto-detection",
                path_str
            ));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Improvement: Use output() to wait for open command completion and capture "app not found" error
        let mut cmd = Command::new("open");
        cmd.args(["-a", "Antigravity"]);

        // Add startup arguments
        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Unable to execute open command: {}", e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Startup failed (open exited with {}): {}",
                output.status, error
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Try to start via registry or default path
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "antigravity://"]);

        // Add startup arguments
        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        let result = cmd.spawn();

        if result.is_err() {
            return Err("Startup failed, please open Antigravity manually".to_string());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("antigravity");

        // Add startup arguments
        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        cmd.spawn().map_err(|e| format!("Startup failed: {}", e))?;
    }

    crate::modules::logger::log_info(&format!(
        "Antigravity startup command sent (default detection, args: {:?})",
        args
    ));
    Ok(())
}

/// Get Antigravity executable path and startup arguments from running processes
///
/// This is the most reliable method to find installations and startup args anywhere
fn get_process_info() -> (Option<std::path::PathBuf>, Option<Vec<String>>) {
    let mut system = System::new_all();
    system.refresh_all();

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        // Exclude manager process itself
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let name = process.name().to_string_lossy().to_lowercase();

        // Get executable path and command line arguments
        if let Some(exe) = process.exe() {
            let mut args = process.cmd().iter();
            let exe_path = args
                .next()
                .map_or(exe.to_string_lossy(), |arg| arg.to_string_lossy())
                .to_lowercase();

            // Extract actual arguments from command line (skipping exe path)
            let args = args
                .map(|arg| arg.to_string_lossy().to_lowercase())
                .collect::<Vec<String>>();

            let args_str = args.join(" ");

            // Common helper process exclusion logic
            let is_helper = args_str.contains("--type=")
                || args_str.contains("node-ipc")
                || args_str.contains("nodeipc")
                || args_str.contains("max-old-space-size")
                || args_str.contains("node_modules")
                || name.contains("helper")
                || name.contains("plugin")
                || name.contains("renderer")
                || name.contains("gpu")
                || name.contains("crashpad")
                || name.contains("utility")
                || name.contains("audio")
                || name.contains("sandbox")
                || exe_path.contains("crashpad");

            let path = Some(exe.to_path_buf());
            let args = Some(args);
            #[cfg(target_os = "macos")]
            {
                // macOS: Exclude helper processes, match main app only, and check Frameworks
                if exe_path.contains("antigravity.app")
                    && !is_helper
                    && !exe_path.contains("frameworks")
                {
                    // Try to extract .app path for better open command support
                    if let Some(app_idx) = exe_path.find(".app") {
                        let app_path_str = &exe.to_string_lossy()[..app_idx + 4];
                        let path = Some(std::path::PathBuf::from(app_path_str));
                        return (path, args);
                    }
                    return (path, args);
                }
            }

            #[cfg(target_os = "windows")]
            {
                // Windows: Strictly match process name and exclude helpers
                if name == "antigravity.exe" && !is_helper {
                    return (path, args);
                }
            }

            #[cfg(target_os = "linux")]
            {
                // Linux: Check process name or path for antigravity, excluding helpers and manager
                if (name == "antigravity" || exe_path.contains("/antigravity"))
                    && !name.contains("tools")
                    && !is_helper
                {
                    return (path, args);
                }
            }
        }
    }
    (None, None)
}

/// Get Antigravity executable path from running processes
///
/// Most reliable method to find installation anywhere
pub fn get_path_from_running_process() -> Option<std::path::PathBuf> {
    let (path, _) = get_process_info();
    path
}

/// Get Antigravity startup arguments from running processes
pub fn get_args_from_running_process() -> Option<Vec<String>> {
    let (_, args) = get_process_info();
    args
}

/// Get --user-data-dir argument value (if exists)
pub fn get_user_data_dir_from_process() -> Option<std::path::PathBuf> {
    // Prefer getting startup arguments from config
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(args) = config.antigravity_args {
            // Check arguments in config
            for i in 0..args.len() {
                if args[i] == "--user-data-dir" && i + 1 < args.len() {
                    // Next argument is the path
                    let path = std::path::PathBuf::from(&args[i + 1]);
                    if path.exists() {
                        return Some(path);
                    }
                } else if args[i].starts_with("--user-data-dir=") {
                    // Argument and value in same string, e.g. --user-data-dir=/path/to/data
                    let parts: Vec<&str> = args[i].splitn(2, '=').collect();
                    if parts.len() == 2 {
                        let path_str = parts[1];
                        let path = std::path::PathBuf::from(path_str);
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    // If not in config, get arguments from running process
    if let Some(args) = get_args_from_running_process() {
        for i in 0..args.len() {
            if args[i] == "--user-data-dir" && i + 1 < args.len() {
                // Next argument is the path
                let path = std::path::PathBuf::from(&args[i + 1]);
                if path.exists() {
                    return Some(path);
                }
            } else if args[i].starts_with("--user-data-dir=") {
                // Argument and value in same string, e.g. --user-data-dir=/path/to/data
                let parts: Vec<&str> = args[i].splitn(2, '=').collect();
                if parts.len() == 2 {
                    let path_str = parts[1];
                    let path = std::path::PathBuf::from(path_str);
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// Get Antigravity executable path (cross-platform)
///
/// Search strategy (highest to lowest priority):
/// 1. Get path from running process (most reliable, supports any location)
/// 2. Iterate standard installation locations
/// 3. Return None
pub fn get_antigravity_executable_path() -> Option<std::path::PathBuf> {
    // Strategy 1: Get from running process (supports any location)
    if let Some(path) = get_path_from_running_process() {
        return Some(path);
    }

    // Strategy 2: Check standard installation locations
    check_standard_locations()
}

/// Check standard installation locations
fn check_standard_locations() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = std::path::PathBuf::from("/Applications/Antigravity.app");
        if path.exists() {
            return Some(path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::env;

        // Get environment variables
        let local_appdata = env::var("LOCALAPPDATA").ok();
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 =
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());

        let mut possible_paths = Vec::new();

        // User installation location (preferred)
        if let Some(local) = local_appdata {
            possible_paths.push(
                std::path::PathBuf::from(&local)
                    .join("Programs")
                    .join("Antigravity")
                    .join("Antigravity.exe"),
            );
        }

        // System installation location
        possible_paths.push(
            std::path::PathBuf::from(&program_files)
                .join("Antigravity")
                .join("Antigravity.exe"),
        );

        // 32-bit compatibility location
        possible_paths.push(
            std::path::PathBuf::from(&program_files_x86)
                .join("Antigravity")
                .join("Antigravity.exe"),
        );

        // Return the first existing path
        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let possible_paths = vec![
            std::path::PathBuf::from("/usr/bin/antigravity"),
            std::path::PathBuf::from("/opt/Antigravity/antigravity"),
            std::path::PathBuf::from("/usr/share/antigravity/antigravity"),
        ];

        // User local installation
        if let Some(home) = dirs::home_dir() {
            let user_local = home.join(".local/bin/antigravity");
            if user_local.exists() {
                return Some(user_local);
            }
        }

        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}

// ============== 实例管理相关函数 ==============

use crate::models::Instance;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Windows: 缓存所有进程的命令行参数
#[cfg(target_os = "windows")]
static PROCESS_CMDLINE_CACHE: Lazy<Mutex<HashMap<u32, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Windows: 使用 wmic 批量获取所有进程的命令行参数
#[cfg(target_os = "windows")]
fn refresh_process_command_line_cache() {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = match Command::new("wmic")
        .args(["process", "get", "ProcessId,CommandLine", "/format:csv"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => o,
        Err(_) => return,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut cache = match PROCESS_CMDLINE_CACHE.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    cache.clear();

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 3 {
            if let Ok(pid) = parts.last().unwrap_or(&"").trim().parse::<u32>() {
                let cmdline = parts[1..parts.len() - 1].join(",");
                if !cmdline.is_empty() {
                    cache.insert(pid, cmdline);
                }
            }
        }
    }
}

/// Windows: 从缓存获取进程命令行参数
#[cfg(target_os = "windows")]
fn get_process_command_line(pid: u32) -> Option<String> {
    PROCESS_CMDLINE_CACHE.lock().ok()?.get(&pid).cloned()
}

/// 非 Windows 平台
#[cfg(not(target_os = "windows"))]
fn get_process_command_line(_pid: u32) -> Option<String> {
    None
}

/// 检查默认实例是否正在运行（使用父进程遍历法）
pub fn is_default_instance_running() -> bool {
    #[cfg(target_os = "windows")]
    refresh_process_command_line_cache();

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let current_pid = std::process::id();

    let is_antigravity_name = |name: &str| -> bool {
        let name_lower = name.to_lowercase();
        name_lower == "antigravity.exe" || name_lower.starts_with("antigravity")
    };

    let find_root_antigravity = |start_pid: sysinfo::Pid| -> Option<sysinfo::Pid> {
        let mut current = start_pid;
        loop {
            let process = system.process(current)?;
            let parent_pid = process.parent()?;

            if let Some(parent) = system.process(parent_pid) {
                let parent_name = parent.name().to_string_lossy();
                if is_antigravity_name(&parent_name) {
                    current = parent_pid;
                    continue;
                }
            }
            return Some(current);
        }
    };

    let mut root_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy();
        if !is_antigravity_name(&name) {
            continue;
        }

        if let Some(root_pid) = find_root_antigravity(*pid) {
            root_pids.insert(root_pid.as_u32());
        }
    }

    for root_pid in root_pids {
        let args_str = {
            #[cfg(target_os = "windows")]
            {
                get_process_command_line(root_pid)
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default()
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Some(process) = system.process(sysinfo::Pid::from_u32(root_pid)) {
                    process
                        .cmd()
                        .iter()
                        .map(|arg| arg.to_string_lossy().to_lowercase())
                        .collect::<Vec<String>>()
                        .join(" ")
                } else {
                    String::new()
                }
            }
        };

        if args_str.is_empty() {
            continue;
        }

        if !args_str.contains("--user-data-dir") {
            crate::modules::logger::log_info(&format!(
                "Found default instance running (no --user-data-dir), root PID: {}",
                root_pid
            ));
            return true;
        }
    }

    false
}

/// 获取实例主进程的命令行参数
pub fn get_instance_root_process_args(user_data_dir: &Path) -> Option<Vec<String>> {
    #[cfg(target_os = "windows")]
    refresh_process_command_line_cache();

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let user_data_dir_str = user_data_dir.to_string_lossy().to_lowercase();
    let user_data_dir_normalized = user_data_dir_str.replace('/', "\\");

    let is_antigravity_name = |name: &str| -> bool {
        let name_lower = name.to_lowercase();
        name_lower == "antigravity.exe" || name_lower.starts_with("antigravity")
    };

    let find_root_antigravity = |start_pid: sysinfo::Pid| -> Option<sysinfo::Pid> {
        let mut current = start_pid;
        loop {
            let process = system.process(current)?;
            let parent_pid = process.parent()?;

            if let Some(parent) = system.process(parent_pid) {
                let parent_name = parent.name().to_string_lossy();
                if is_antigravity_name(&parent_name) {
                    current = parent_pid;
                    continue;
                }
            }
            return Some(current);
        }
    };

    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy();
        if !is_antigravity_name(&name) {
            continue;
        }

        let args_str = {
            #[cfg(target_os = "windows")]
            {
                get_process_command_line(pid.as_u32())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default()
            }
            #[cfg(not(target_os = "windows"))]
            {
                process
                    .cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy().to_lowercase())
                    .collect::<Vec<String>>()
                    .join(" ")
            }
        };

        let args_normalized = args_str.replace('/', "\\");
        if !args_normalized.contains(&user_data_dir_normalized) {
            continue;
        }

        if let Some(root_pid) = find_root_antigravity(*pid) {
            #[cfg(target_os = "windows")]
            {
                if let Some(cmdline) = get_process_command_line(root_pid.as_u32()) {
                    let args = parse_cmdline_to_args(&cmdline);
                    if !args.is_empty() {
                        crate::modules::logger::log_info(&format!(
                            "Got root process args for instance, PID: {}, args count: {}",
                            root_pid.as_u32(),
                            args.len()
                        ));
                        return Some(args);
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Some(root_process) = system.process(root_pid) {
                    let args: Vec<String> = root_process
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_string())
                        .collect();
                    if !args.is_empty() {
                        return Some(args);
                    }
                }
            }
        }
    }

    None
}

/// 解析命令行字符串为参数列表
fn parse_cmdline_to_args(cmdline: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in cmdline.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// 获取特定实例的所有进程 PID
pub fn get_instance_pids(user_data_dir: &Path) -> Vec<u32> {
    #[cfg(target_os = "windows")]
    refresh_process_command_line_cache();

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();
    let user_data_str = user_data_dir.to_string_lossy().to_lowercase();

    let mut pids = Vec::new();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_antigravity = {
            #[cfg(target_os = "macos")]
            {
                exe_path.contains("antigravity.app")
            }
            #[cfg(target_os = "windows")]
            {
                name == "antigravity.exe" || name.starts_with("antigravity")
            }
            #[cfg(target_os = "linux")]
            {
                name.contains("antigravity") || exe_path.contains("/antigravity")
            }
        };

        if !is_antigravity {
            continue;
        }

        let args_str = {
            #[cfg(target_os = "windows")]
            {
                get_process_command_line(pid_u32)
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default()
            }
            #[cfg(not(target_os = "windows"))]
            {
                process
                    .cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy().to_lowercase())
                    .collect::<Vec<String>>()
                    .join(" ")
            }
        };

        let normalized_args = args_str.replace('/', "\\");
        let normalized_target = user_data_str.replace('/', "\\");

        if normalized_args.contains(&normalized_target) {
            pids.push(pid_u32);
        }
    }

    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "Found {} processes for instance with user_data_dir: {:?}",
            pids.len(),
            user_data_dir
        ));
    }

    pids
}

/// 检查实例是否正在运行
pub fn is_instance_running(user_data_dir: &Path) -> bool {
    !get_instance_pids(user_data_dir).is_empty()
}

/// 关闭实例（关闭所有对应的主进程，让 Chromium 优雅关闭子进程）
pub fn close_instance(user_data_dir: &Path, _timeout_secs: u64) -> Result<(), String> {
    // 获取所有主进程 PID（支持多窗口情况）
    let root_pids = get_all_instance_root_pids(user_data_dir);

    if root_pids.is_empty() {
        crate::modules::logger::log_info("Instance not running, nothing to close");
        return Ok(());
    }

    crate::modules::logger::log_info(&format!(
        "Closing instance main processes, PIDs: {:?}",
        root_pids
    ));

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        for pid in &root_pids {
            // 使用 /F 强制关闭主进程
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        for pid in &root_pids {
            // 发送 SIGTERM 而非 SIGKILL，让进程优雅关闭
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
        }
    }

    // 等待进程优雅关闭
    thread::sleep(Duration::from_millis(1000));
    Ok(())
}

/// 获取所有实例的主进程 PID（使用父进程遍历法）
///
/// 逻辑：
/// 1. 找到所有 antigravity.exe 进程
/// 2. 对每个进程，向上遍历父进程直到顶层（父进程不再是 antigravity）
/// 3. 收集所有唯一的顶层进程（根进程）
/// 4. 只检查根进程的命令行来判断属于哪个实例
fn get_all_instance_root_pids(user_data_dir: &Path) -> Vec<u32> {
    #[cfg(target_os = "windows")]
    refresh_process_command_line_cache();

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let user_data_dir_str = user_data_dir.to_string_lossy().to_lowercase();
    let user_data_dir_normalized = user_data_dir_str.replace('/', "\\");

    // 检查是否是默认数据目录（没有自定义 --user-data-dir 的实例）
    let default_user_data = dirs::data_local_dir()
        .map(|p| p.join("Antigravity"))
        .unwrap_or_default();
    let is_default_dir = user_data_dir == default_user_data
        || user_data_dir_str.contains("antigravity") && !user_data_dir_str.contains("antigravity-");

    crate::modules::logger::log_info(&format!(
        "[close_debug] user_data_dir={}, is_default_dir={}",
        user_data_dir.display(),
        is_default_dir
    ));

    let is_antigravity_name = |name: &str| -> bool {
        let name_lower = name.to_lowercase();
        name_lower == "antigravity.exe" || name_lower == "antigravity"
    };

    // 第一步：找到所有 antigravity 进程的根进程
    let find_root_antigravity = |start_pid: sysinfo::Pid| -> Option<sysinfo::Pid> {
        let mut current = start_pid;
        loop {
            let process = system.process(current)?;
            let parent_pid = process.parent()?;

            if let Some(parent) = system.process(parent_pid) {
                let parent_name = parent.name().to_string_lossy();
                if is_antigravity_name(&parent_name) {
                    current = parent_pid;
                    continue;
                }
            }
            return Some(current);
        }
    };

    // 收集所有唯一的根进程
    let mut all_roots: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy();
        if !is_antigravity_name(&name) {
            continue;
        }

        if let Some(root_pid) = find_root_antigravity(*pid) {
            all_roots.insert(root_pid.as_u32());
        }
    }

    crate::modules::logger::log_info(&format!(
        "[close_debug] Found {} unique root processes: {:?}",
        all_roots.len(),
        all_roots
    ));

    // 第二步：检查每个根进程的命令行，判断是否属于目标实例
    let mut matching_roots: Vec<u32> = Vec::new();

    for root_pid in all_roots {
        let args_str = {
            #[cfg(target_os = "windows")]
            {
                get_process_command_line(root_pid)
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default()
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Some(process) = system.process(sysinfo::Pid::from_u32(root_pid)) {
                    process
                        .cmd()
                        .iter()
                        .map(|arg| arg.to_string_lossy().to_lowercase())
                        .collect::<Vec<String>>()
                        .join(" ")
                } else {
                    String::new()
                }
            }
        };

        let args_normalized = args_str.replace('/', "\\");

        // 匹配逻辑：
        // 1. 如果是默认目录，匹配没有 --user-data-dir 参数的进程
        // 2. 否则匹配包含指定目录的进程
        let matches = if is_default_dir {
            !args_normalized.contains("--user-data-dir=")
        } else {
            args_normalized.contains(&user_data_dir_normalized)
        };

        crate::modules::logger::log_info(&format!(
            "[close_debug] Root PID={}, matches={}, has_user_data_dir={}, args(first 200)={}",
            root_pid,
            matches,
            args_normalized.contains("--user-data-dir="),
            &args_normalized[..args_normalized.len().min(200)]
        ));

        if matches {
            matching_roots.push(root_pid);
        }
    }

    crate::modules::logger::log_info(&format!(
        "[close_debug] Final matching root PIDs: {:?}",
        matching_roots
    ));

    matching_roots
}

/// 获取实例的主进程 PID（顶层 Antigravity 进程）
fn get_instance_root_pid(user_data_dir: &Path) -> Option<u32> {
    #[cfg(target_os = "windows")]
    refresh_process_command_line_cache();

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let user_data_dir_str = user_data_dir.to_string_lossy().to_lowercase();
    let user_data_dir_normalized = user_data_dir_str.replace('/', "\\");

    let is_antigravity_name = |name: &str| -> bool {
        let name_lower = name.to_lowercase();
        // 只匹配 antigravity.exe，排除 antigravity_tools.exe 等
        name_lower == "antigravity.exe" || name_lower == "antigravity"
    };

    let find_root_antigravity = |start_pid: sysinfo::Pid| -> Option<sysinfo::Pid> {
        let mut current = start_pid;
        loop {
            let process = system.process(current)?;
            let parent_pid = process.parent()?;

            if let Some(parent) = system.process(parent_pid) {
                let parent_name = parent.name().to_string_lossy();
                if is_antigravity_name(&parent_name) {
                    current = parent_pid;
                    continue;
                }
            }
            return Some(current);
        }
    };

    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy();
        if !is_antigravity_name(&name) {
            continue;
        }

        let args_str = {
            #[cfg(target_os = "windows")]
            {
                get_process_command_line(pid.as_u32())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default()
            }
            #[cfg(not(target_os = "windows"))]
            {
                process
                    .cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy().to_lowercase())
                    .collect::<Vec<String>>()
                    .join(" ")
            }
        };

        let args_normalized = args_str.replace('/', "\\");
        if !args_normalized.contains(&user_data_dir_normalized) {
            continue;
        }

        if let Some(root_pid) = find_root_antigravity(*pid) {
            return Some(root_pid.as_u32());
        }
    }

    None
}

/// 启动实例
pub fn start_instance(instance: &Instance) -> Result<(), String> {
    let exe_path = instance
        .antigravity_executable
        .clone()
        .or_else(|| get_antigravity_executable_path().map(|p| p.to_string_lossy().to_string()))
        .ok_or("Cannot find Antigravity executable")?;

    let args = instance.get_launch_args();

    crate::modules::logger::log_info(&format!(
        "Starting instance {} with args: {:?}",
        instance.name, args
    ));

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new(&exe_path)
            .args(&args)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to start instance: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new(&exe_path)
            .args(&args)
            .spawn()
            .map_err(|e| format!("Failed to start instance: {}", e))?;
    }

    crate::modules::logger::log_info(&format!("Instance startup command sent: {}", instance.name));
    Ok(())
}

/// 使用指定参数启动实例
pub fn start_instance_with_args(instance: &Instance, args: Vec<String>) -> Result<(), String> {
    let exe_path = instance
        .antigravity_executable
        .clone()
        .or_else(|| get_antigravity_executable_path().map(|p| p.to_string_lossy().to_string()))
        .ok_or("Cannot find Antigravity executable")?;

    crate::modules::logger::log_info(&format!(
        "Starting instance {} with custom args: {:?}",
        instance.name, args
    ));

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new(&exe_path)
            .args(&args)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to start instance: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new(&exe_path)
            .args(&args)
            .spawn()
            .map_err(|e| format!("Failed to start instance: {}", e))?;
    }

    crate::modules::logger::log_info(&format!(
        "Instance startup command sent: {} (with saved args)",
        instance.name
    ));
    Ok(())
}

/// 重启实例
pub fn restart_instance(instance: &Instance, timeout_secs: u64) -> Result<(), String> {
    close_instance(&instance.user_data_dir, timeout_secs)?;
    thread::sleep(Duration::from_secs(1));
    start_instance(instance)
}
