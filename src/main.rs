use anyhow::{anyhow, Result};
use nix::libc::uid_t;
use nix::mount::{mount, umount, MsFlags};
use nix::sched::{clone, unshare, CloneFlags};
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{chroot, getuid, sethostname};
use nix::unistd::{Uid, User};
use rlimit::{getrlimit, Resource};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_STACK_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug)]
struct MountPoint {
    source: PathBuf,
    target: PathBuf,
    fstype: String,
    flags: MsFlags,
}

impl MountPoint {
    fn new(src: &str, tgt: &str, fstype: &str, flags: MsFlags) -> Self {
        Self {
            source: PathBuf::from(src),
            target: PathBuf::from(tgt),
            fstype: String::from(fstype),
            flags,
        }
    }

    fn mount(&self) -> Result<()> {
        println!("debug: {:?}", self);
        mount(
            Some(self.source.as_path()),
            self.target.as_path(),
            Some(self.fstype.as_str()),
            self.flags,
            None::<&str>,
        )
        .map_err(|e| anyhow!(e))
    }

    fn umount(&self) -> Result<()> {
        umount(self.target.as_path()).map_err(|e| anyhow!(e))
    }
}

impl Drop for MountPoint {
    fn drop(&mut self) {
        self.umount().expect("umount failed");
    }
}

fn cgroups() -> Result<()> {
    let pid = std::process::id().to_string();
    let cgroup_dir = Path::new("/sys/fs/cgroup/pids/ctr");
    fs::create_dir_all(cgroup_dir)?;
    for (filename, content) in [
        ("pids.max", "20".as_bytes()),
        ("notify_on_release", "1".as_bytes()),
        ("cgroup.procs", pid.as_bytes()),
    ] {
        let path = Path::join(cgroup_dir, filename);
        let mut file = File::create(path)?;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o700);
        file.write_all(content)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let stack_size =
        getrlimit(Resource::STACK).map_or_else(|_| DEFAULT_STACK_SIZE, |v| v.0 as usize);
    let mut stack = vec![0; stack_size];
    let flags = CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWPID | CloneFlags::CLONE_NEWNS;

    let pid = clone(
        Box::new(|| {
            let mut rootfs = User::from_uid(env::var("SUDO_UID").map_or(getuid(), |u| {
                Uid::from_raw(u.parse::<u32>().unwrap() as uid_t)
            }))
            .unwrap()
            .expect("cannot determine users home directory")
            .dir;
            rootfs.push("rootfs-x86_64");
            unshare(CloneFlags::CLONE_NEWNS).expect("unable to unshare CLONE_NEWNS");
            cgroups().expect("failed to setup cgroups");
            sethostname(OsStr::new("ctr")).expect("unable to set hostname");
            chroot(rootfs.as_path()).expect("unable to chroot to rootfs");
            env::set_current_dir(Path::new("/")).expect("unable to change working directory");
            let procfs = MountPoint::new("/proc", "/proc", "proc", MsFlags::empty());
            procfs.mount().expect("unable to mount proc fs");

            let (cmd, cmd_args) = args[1..].split_at(1);
            Command::new(OsStr::new(&cmd[0]))
                .args(cmd_args)
                .env_clear()
                .current_dir("/")
                .spawn()
                .expect("command invocation failed")
                .wait()
                .map_or(-1, |x| x.code().unwrap_or(-1) as isize)
        }),
        &mut stack,
        flags,
        Some(signal::SIGCHLD as i32),
    )?;

    waitpid(pid, None).map_or_else(
        |e| Err(anyhow::Error::new(e)),
        |v| match v {
            WaitStatus::Exited(_pid, _rc) => Ok(()),
            _ => Err(anyhow!("failed to exit cleanly")),
        },
    )
}
