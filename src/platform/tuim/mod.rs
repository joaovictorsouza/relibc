#![allow(unused_imports, unused_variables, deprecated)]
use core::{ptr, slice, num::NonZeroU64};
use crate::ld_so::tcb::OsSpecific;
use crate::header::{sys_time::itimerval, sys_mman::{PROT_READ, PROT_WRITE}};
use super::{Pal, PalEpoll, PalPtrace, PalSignal, PalSocket, types::*};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        bits_sigset_t::sigset_t,
        bits_pthread::*,
        errno::*,
        fcntl::*,
        signal::{sigaction, siginfo_t, sigval, stack_t, sigevent},
        sys_epoll::epoll_event,
        sys_resource::{rlimit, rusage},
        sys_select::timeval,
        sys_socket::{msghdr, sockaddr, socklen_t},
        sys_stat::stat,
        sys_statvfs::statvfs,
        sys_time::timezone,
        sys_utsname::utsname,
        time::{itimerspec, timespec},
        unistd::*,
    },
    out::Out,
};

pub struct Sys;

impl Sys {
    pub unsafe fn ioctl(fd: c_int, request: c_ulong, out: *mut c_void) -> Result<c_int> {
        Ok(0)
    }
}

// Dummy Syscall numbers max
const ERRNO_MAX: usize = 4095;

fn e(r: i64) -> Result<usize> {
    if r < 0 {
        Err(Errno(-r as i32))
    } else {
        Ok(r as usize)
    }
}

static mut CURRENT_DIR: [u8; 512] = {
    let mut buf = [0u8; 512];
    buf[0] = b'/';
    buf
};

impl Pal for Sys {
    fn faccessat(fd: c_int, path: CStr, amode: c_int, flags: c_int) -> Result<()> {
        Ok(())
    }

    unsafe fn brk(addr: *mut c_void) -> Result<*mut c_void> {
        unsafe {
            static mut BRK_CUR: *mut c_void = ptr::null_mut();
            static mut BRK_END: *mut c_void = ptr::null_mut();
            if BRK_CUR.is_null() {
                const BRK_MAX_SIZE: usize = 4 * 1024 * 1024;
                let allocated = Self::mmap(
                    ptr::null_mut(),
                    BRK_MAX_SIZE,
                    PROT_READ | PROT_WRITE,
                    0, // flags
                    -1,
                    0,
                )?;
                BRK_CUR = allocated;
                BRK_END = (allocated as *mut u8).add(BRK_MAX_SIZE) as *mut c_void;
            }
            if addr.is_null() {
                Ok(BRK_CUR)
            } else if BRK_CUR <= addr && addr < BRK_END {
                BRK_CUR = addr;
                Ok(addr)
            } else {
                Err(Errno(ENOMEM))
            }
        }
    }

    fn chdir(path: CStr) -> Result<()> {
        let bytes = path.to_bytes();
        if bytes.is_empty() {
            return Err(Errno(EINVAL));
        }

        const CWD_MAX: usize = 512;
        unsafe {
            let current_dir_ptr = ptr::addr_of_mut!(CURRENT_DIR) as *mut u8;
            if bytes[0] == b'/' {
                // Absolute path
                if bytes.len() >= CWD_MAX {
                    return Err(Errno(ENAMETOOLONG));
                }
                ptr::copy_nonoverlapping(bytes.as_ptr(), current_dir_ptr, bytes.len());
                ptr::write_bytes(current_dir_ptr.add(bytes.len()), 0, CWD_MAX - bytes.len());
            } else {
                // Relative path
                let mut len = 0;
                while len < CWD_MAX && *current_dir_ptr.add(len) != 0 {
                    len += 1;
                }
                let add_slash = if len > 0 && *current_dir_ptr.add(len - 1) != b'/' { 1 } else { 0 };
                let needed = len + add_slash + bytes.len();
                if needed >= CWD_MAX {
                    return Err(Errno(ENAMETOOLONG));
                }
                if add_slash > 0 {
                    *current_dir_ptr.add(len) = b'/';
                }
                ptr::copy_nonoverlapping(bytes.as_ptr(), current_dir_ptr.add(len + add_slash), bytes.len());
                ptr::write_bytes(current_dir_ptr.add(needed), 0, CWD_MAX - needed);
            }
        }
        Ok(())
    }

    fn clock_getres(clk_id: clockid_t, res: Option<Out<timespec>>) -> Result<()> {
        if let Some(mut tp) = res {
            tp.write(timespec { tv_sec: 0, tv_nsec: 1000000 });
        }
        Ok(())
    }

    fn clock_gettime(clk_id: clockid_t, mut tp: Out<timespec>) -> Result<()> {
        let mut ts = libtuim::abi::Timespec::default();
        let r = libtuim::sys::clock_get(clk_id as u32, &mut ts);
        if r < 0 {
            return Err(Errno(-r as i32));
        }
        tp.write(timespec {
            tv_sec: ts.tv_sec,
            tv_nsec: ts.tv_nsec,
        });
        Ok(())
    }

    unsafe fn clock_settime(clk_id: clockid_t, tp: *const timespec) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn close(fildes: c_int) -> Result<()> {
        let r = libfdio::fdio_close(fildes);
        if r < 0 {
            Err(Errno(-r))
        } else {
            Ok(())
        }
    }

    fn dup(fildes: c_int) -> Result<c_int> {
        let r = libfdio::fdio_dup(fildes);
        if r < 0 {
            Err(Errno(-r))
        } else {
            Ok(r)
        }
    }

    fn dup2(fildes: c_int, fildes2: c_int) -> Result<c_int> {
        let r = libfdio::fdio_dup2(fildes, fildes2);
        if r < 0 {
            Err(Errno(-r))
        } else {
            Ok(r)
        }
    }

    unsafe fn execve(path: CStr, argv: *const *mut c_char, envp: *const *mut c_char) -> Result<()> {
        let path_str = path.to_str().map_err(|_| Errno(EINVAL))?;
        let r = libtuim::sys::spawn(path_str);
        if r < 0 {
            return Err(Errno(-r as i32));
        }
        libtuim::sys::exit(0);
    }

    unsafe fn fexecve(
        fildes: c_int,
        argv: *const *mut c_char,
        envp: *const *mut c_char,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn exit(status: c_int) -> ! {
        libtuim::sys::exit(status)
    }

    unsafe fn exit_thread(stack_base: *mut (), stack_size: usize) -> ! {
        libtuim::sys::exit(0)
    }

    fn fchdir(fildes: c_int) -> Result<()> {
        Ok(())
    }

    fn fchmodat(dirfd: c_int, path: Option<CStr>, mode: mode_t, flags: c_int) -> Result<()> {
        Ok(())
    }

    fn fchownat(fildes: c_int, path: CStr, owner: uid_t, group: gid_t, flags: c_int) -> Result<()> {
        Ok(())
    }

    fn fcntl(fd: c_int, cmd: c_int, args: c_ulonglong) -> Result<c_int> {
        Ok(0)
    }

    fn fdatasync(fd: c_int) -> Result<()> {
        Ok(())
    }

    fn flock(fd: c_int, operation: c_int) -> Result<()> {
        Ok(())
    }

    unsafe fn fork() -> Result<pid_t> {
        Err(Errno(ENOSYS))
    }

    fn fpath(fildes: c_int, out: &mut [u8]) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn fsync(fd: c_int) -> Result<()> {
        Ok(())
    }

    fn ftruncate(fd: c_int, len: off_t) -> Result<()> {
        Ok(())
    }

    unsafe fn futex_wait(addr: *mut u32, val: u32, deadline: Option<&timespec>) -> Result<()> {
        let r = unsafe { libtuim::sys::futex_wait(addr, val) };
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(())
        }
    }

    unsafe fn futex_wake(addr: *mut u32, num: u32) -> Result<u32> {
        let r = unsafe { libtuim::sys::futex_wake(addr, num) };
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(r as u32)
        }
    }

    unsafe fn utimensat(
        dirfd: c_int,
        path: CStr,
        times: *const timespec,
        flag: c_int,
    ) -> Result<()> {
        Ok(())
    }

    fn getcwd(mut buf: Out<[u8]>) -> Result<()> {
        const CWD_MAX: usize = 512;
        unsafe {
            let current_dir_ptr = ptr::addr_of!(CURRENT_DIR) as *const u8;
            let mut len = 0;
            while len < CWD_MAX && *current_dir_ptr.add(len) != 0 {
                len += 1;
            }
            if buf.len() < len + 1 {
                return Err(Errno(ERANGE));
            }
            let dst = buf.as_mut_ptr().as_mut_ptr();
            ptr::copy_nonoverlapping(current_dir_ptr, dst, len);
            dst.add(len).write(0);
        }
        Ok(())
    }

    fn getdents(fd: c_int, buf: &mut [u8], opaque: u64) -> Result<usize> {
        Ok(0)
    }

    fn dir_seek(fd: c_int, off: u64) -> Result<()> {
        Ok(())
    }

    unsafe fn dent_reclen_offset(this_dent: &[u8], offset: usize) -> Option<(u16, u64)> {
        None
    }

    fn getegid() -> gid_t { 0 }
    fn geteuid() -> uid_t { 0 }
    fn getgid() -> gid_t { 0 }
    fn getgroups(list: Out<[gid_t]>) -> Result<c_int> { Ok(0) }
    fn getpagesize() -> usize { 16384 }
    fn getpgid(pid: pid_t) -> Result<pid_t> { Ok(0) }
    fn getpid() -> pid_t { libtuim::sys::getpid() as pid_t }
    fn getppid() -> pid_t { 0 }
    fn getpriority(which: c_int, who: id_t) -> Result<c_int> { Ok(0) }
    fn getrandom(buf: &mut [u8], flags: c_uint) -> Result<usize> {
        for x in buf.iter_mut() {
            *x = 0x42;
        }
        Ok(buf.len())
    }

    fn getresgid(
        rgid: Option<Out<gid_t>>,
        egid: Option<Out<gid_t>>,
        sgid: Option<Out<gid_t>>,
    ) -> Result<()> {
        if let Some(mut r) = rgid { r.write(0); }
        if let Some(mut e) = egid { e.write(0); }
        if let Some(mut s) = sgid { s.write(0); }
        Ok(())
    }

    fn getresuid(
        ruid: Option<Out<uid_t>>,
        euid: Option<Out<uid_t>>,
        suid: Option<Out<uid_t>>,
    ) -> Result<()> {
        if let Some(mut r) = ruid { r.write(0); }
        if let Some(mut e) = euid { e.write(0); }
        if let Some(mut s) = suid { s.write(0); }
        Ok(())
    }

    fn getrlimit(resource: c_int, rlim: Out<rlimit>) -> Result<()> {
        Ok(())
    }

    unsafe fn setrlimit(resource: c_int, rlim: *const rlimit) -> Result<()> {
        Ok(())
    }

    fn getrusage(who: c_int, r_usage: Out<rusage>) -> Result<()> {
        Ok(())
    }

    fn getsid(pid: pid_t) -> Result<pid_t> { Ok(0) }
    fn gettid() -> pid_t { libtuim::sys::gettid() as pid_t }

    fn gettimeofday(mut tp: Out<timeval>, tzp: Option<Out<timezone>>) -> Result<()> {
        let mut ts = libtuim::abi::Timespec::default();
        let r = libtuim::sys::clock_get(libtuim::abi::CLOCK_REALTIME, &mut ts);
        if r < 0 {
            return Err(Errno(-r as i32));
        }
        tp.write(timeval {
            tv_sec: ts.tv_sec,
            tv_usec: (ts.tv_nsec / 1000) as suseconds_t,
        });
        if let Some(mut tz) = tzp {
            tz.write(timezone {
                tz_minuteswest: 0,
                tz_dsttime: 0,
            });
        }
        Ok(())
    }

    fn getuid() -> uid_t { 0 }

    fn linkat(fd1: c_int, path1: CStr, fd2: c_int, path2: CStr, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn lseek(fd: c_int, offset: off_t, whence: c_int) -> Result<off_t> {
        let r = libfdio::fdio_lseek(fd, offset, whence);
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(r)
        }
    }

    fn mkdirat(dir_fd: c_int, path: CStr, mode: mode_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn mknodat(dir_fd: c_int, path: CStr, mode: mode_t, dev: dev_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn mkfifoat(dir_fd: c_int, path: CStr, mode: mode_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn mlock(addr: *const c_void, len: usize) -> Result<()> { Ok(()) }
    unsafe fn mlockall(flags: c_int) -> Result<()> { Ok(()) }

    unsafe fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fildes: c_int,
        off: off_t,
    ) -> Result<*mut c_void> {
        let r = libtuim::sys::mmap(addr as usize, len);
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(r as *mut c_void)
        }
    }

    unsafe fn mremap(
        addr: *mut c_void,
        len: usize,
        new_len: usize,
        flags: c_int,
        args: *mut c_void,
    ) -> Result<*mut c_void> {
        Err(Errno(ENOSYS))
    }

    unsafe fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> Result<()> {
        let r = libtuim::sys::mprotect(addr as usize, len, prot as u32);
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(())
        }
    }

    unsafe fn msync(addr: *mut c_void, len: usize, flags: c_int) -> Result<()> { Ok(()) }
    unsafe fn munlock(addr: *const c_void, len: usize) -> Result<()> { Ok(()) }
    unsafe fn munlockall() -> Result<()> { Ok(()) }

    unsafe fn munmap(addr: *mut c_void, len: usize) -> Result<()> {
        let r = libtuim::sys::munmap(addr as usize, len);
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(())
        }
    }

    unsafe fn madvise(addr: *mut c_void, len: usize, flags: c_int) -> Result<()> { Ok(()) }

    unsafe fn nanosleep(rqtp: *const timespec, rmtp: *mut timespec) -> Result<()> {
        libtuim::sys::yield_now();
        Ok(())
    }

    fn openat(dirfd: c_int, path: CStr, oflag: c_int, mode: mode_t) -> Result<c_int> {
        let path_bytes = path.to_bytes();
        let r = unsafe { libfdio::fdio_open(path_bytes.as_ptr(), path_bytes.len()) };
        if r < 0 {
            Err(Errno(-r))
        } else {
            Ok(r)
        }
    }

    fn pipe2(mut fildes: Out<[c_int; 2]>, flags: c_int) -> Result<()> {
        let mut fds = [0i32; 2];
        let r = unsafe { libfdio::fdio_pipe(fds.as_mut_ptr()) };
        if r < 0 {
            Err(Errno(-r))
        } else {
            fildes.write(fds);
            Ok(())
        }
    }

    fn posix_fallocate(fd: c_int, offset: u64, length: NonZeroU64) -> Result<()> { Ok(()) }
    fn posix_getdents(fildes: c_int, buf: &mut [u8]) -> Result<usize> { Ok(0) }

    unsafe fn rlct_clone(
        stack: *mut usize,
        os_specific: &mut OsSpecific,
    ) -> Result<crate::pthread::OsTid, Errno> {
        Err(Errno(ENOSYS))
    }

    unsafe fn rlct_kill(os_tid: crate::pthread::OsTid, signal: usize) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn current_os_tid() -> crate::pthread::OsTid {
        crate::pthread::OsTid { thread_id: 0 }
    }

    fn read(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        let r = unsafe { libfdio::fdio_read(fildes, buf.as_mut_ptr(), buf.len()) };
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(r as usize)
        }
    }

    fn pread(fildes: c_int, buf: &mut [u8], offset: off_t) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn readlinkat(dirfd: c_int, pathname: CStr, out: &mut [u8]) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn renameat2(
        old_dir: c_int,
        old_path: CStr,
        new_dir: c_int,
        new_path: CStr,
        flags: c_uint,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sched_yield() -> Result<()> {
        libtuim::sys::yield_now();
        Ok(())
    }

    unsafe fn setgroups(size: size_t, list: *const gid_t) -> Result<()> { Ok(()) }
    fn setpgid(pid: pid_t, pgid: pid_t) -> Result<()> { Ok(()) }
    fn setpriority(which: c_int, who: id_t, prio: c_int) -> Result<()> { Ok(()) }
    fn setresgid(rgid: gid_t, egid: gid_t, sgid: gid_t) -> Result<()> { Ok(()) }
    fn setresuid(ruid: uid_t, euid: uid_t, suid: uid_t) -> Result<()> { Ok(()) }
    fn setsid() -> Result<c_int> { Ok(0) }

    fn symlinkat(path1: CStr, fd: c_int, path2: CStr) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sync() -> Result<()> { Ok(()) }

    fn timer_create(clock_id: clockid_t, evp: &sigevent, timerid: Out<timer_t>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn timer_delete(timerid: timer_t) -> Result<()> { Err(Errno(ENOSYS)) }
    fn timer_gettime(timerid: timer_t, value: Out<itimerspec>) -> Result<()> { Err(Errno(ENOSYS)) }
    fn timer_settime(
        timerid: timer_t,
        flags: c_int,
        value: &itimerspec,
        ovalue: Option<Out<itimerspec>>,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn umask(mask: mode_t) -> mode_t { 0 }

    fn uname(utsname: Out<utsname>) -> Result<()> {
        Ok(())
    }

    fn unlinkat(fd: c_int, path: CStr, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn waitpid(pid: pid_t, stat_loc: Option<Out<c_int>>, options: c_int) -> Result<pid_t> {
        let r = libtuim::sys::thread_join(pid as u32);
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            if let Some(mut stat) = stat_loc {
                stat.write(0);
            }
            Ok(pid)
        }
    }

    fn write(fildes: c_int, buf: &[u8]) -> Result<usize> {
        let r = unsafe { libfdio::fdio_write(fildes, buf.as_ptr(), buf.len()) };
        if r < 0 {
            Err(Errno(-r as i32))
        } else {
            Ok(r as usize)
        }
    }

    fn pwrite(fildes: c_int, buf: &[u8], off: off_t) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn verify() -> bool {
        true
    }

    fn fstatat(fildes: c_int, path: Option<CStr>, mut buf: Out<stat>, flags: c_int) -> Result<()> {
        buf.write(stat {
            st_dev: 0,
            st_ino: 0,
            st_mode: 0o100644,
            st_nlink: 1,
            st_uid: 0,
            st_gid: 0,
            st_rdev: 0,
            st_size: 0,
            st_blksize: 4096,
            st_blocks: 0,
            st_atim: timespec { tv_sec: 0, tv_nsec: 0 },
            st_mtim: timespec { tv_sec: 0, tv_nsec: 0 },
            st_ctim: timespec { tv_sec: 0, tv_nsec: 0 },
            ..Default::default()
        });
        Ok(())
    }

    fn fstatvfs(fildes: c_int, mut buf: Out<statvfs>) -> Result<()> {
        buf.write(statvfs {
            f_bsize: 4096,
            f_frsize: 4096,
            f_blocks: 0,
            f_bfree: 0,
            f_bavail: 0,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_fsid: 0,
            f_flag: 0,
            f_namemax: 255,
        });
        Ok(())
    }
}

impl PalEpoll for Sys {
    fn epoll_create1(flags: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    unsafe fn epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut epoll_event) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn epoll_pwait(
        epfd: c_int,
        events: *mut epoll_event,
        maxevents: c_int,
        timeout: c_int,
        sigmask: *const sigset_t,
    ) -> Result<usize> {
        Err(Errno(ENOSYS))
    }
}

impl PalPtrace for Sys {
    unsafe fn ptrace(
        request: c_int,
        pid: pid_t,
        addr: *mut c_void,
        data: *mut c_void,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }
}

impl PalSignal for Sys {
    fn getitimer(which: c_int, out: &mut itimerval) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn kill(pid: pid_t, sig: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigqueue(pid: pid_t, sig: c_int, val: sigval) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn killpg(pgrp: pid_t, sig: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn raise(sig: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setitimer(which: c_int, new: &itimerval, old: Option<&mut itimerval>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigaction(sig: c_int, act: Option<&sigaction>, oact: Option<&mut sigaction>) -> Result<()> {
        Ok(())
    }

    unsafe fn sigaltstack(ss: Option<&stack_t>, old_ss: Option<&mut stack_t>) -> Result<()> {
        Ok(())
    }

    fn sigpending(set: &mut sigset_t) -> Result<()> {
        Ok(())
    }

    fn sigprocmask(how: c_int, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) -> Result<()> {
        Ok(())
    }

    fn sigsuspend(mask: &sigset_t) -> Errno {
        Errno(EINTR)
    }

    fn sigtimedwait(
        set: &sigset_t,
        sig: Option<&mut siginfo_t>,
        tp: Option<&timespec>,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }
}

impl PalSocket for Sys {
    unsafe fn accept(
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    unsafe fn bind(socket: c_int, address: *const sockaddr, address_len: socklen_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn connect(
        socket: c_int,
        address: *const sockaddr,
        address_len: socklen_t,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    unsafe fn getpeername(
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn getsockname(
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn getsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: *mut c_void,
        option_len: *mut socklen_t,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn listen(socket: c_int, backlog: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn recvfrom(
        socket: c_int,
        buf: *mut c_void,
        len: size_t,
        flags: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn recvmsg(socket: c_int, msg: *mut msghdr, flags: c_int) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn sendmsg(socket: c_int, msg: *const msghdr, flags: c_int) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn sendto(
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
        dest_addr: *const sockaddr,
        dest_len: socklen_t,
    ) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn setsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: *const c_void,
        option_len: socklen_t,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn shutdown(socket: c_int, how: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn socket(domain: c_int, kind: c_int, protocol: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn socketpair(domain: c_int, kind: c_int, protocol: c_int, sv: &mut [c_int; 2]) -> Result<()> {
        Err(Errno(ENOSYS))
    }
}
