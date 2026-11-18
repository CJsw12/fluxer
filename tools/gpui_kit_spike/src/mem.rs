//! 프로세스 메모리 조회 (Windows 전용 FFI 한 곳만 unsafe).

#[derive(Clone, Copy, Debug, Default)]
pub struct MemInfo {
    pub peak_working_set: u64,
    pub private_usage: u64,
}

#[cfg(windows)]
pub fn process_memory() -> anyhow::Result<MemInfo> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: PROCESS_MEMORY_COUNTERS_EX는 정수만 가진 C 구조체라 0 초기화가 유효하다.
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: c는 cb 크기의 유효한 EX 구조체이고 EX는 PROCESS_MEMORY_COUNTERS를 접두로 가지므로
    // 포인터 캐스트가 유효하다. GetCurrentProcess의 의사 핸들은 닫을 필요가 없다.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            c.cb,
        )
    };
    if ok == 0 {
        anyhow::bail!("GetProcessMemoryInfo failed");
    }
    Ok(MemInfo {
        peak_working_set: c.PeakWorkingSetSize as u64,
        private_usage: c.PrivateUsage as u64,
    })
}

#[cfg(not(windows))]
pub fn process_memory() -> anyhow::Result<MemInfo> {
    anyhow::bail!("process memory is only implemented on Windows")
}
