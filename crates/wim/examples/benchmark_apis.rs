//! Matched Linux workloads through the safe Rust API and the C interface.
#[cfg(target_os = "linux")]
mod linux {
    use std::{ffi::CString, hint::black_box, path::Path, ptr::NonNull, time::Instant};
    use wim::{Compression, ImageIndex, OpenOptions, Wim, ffi};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    trait Archive: Sized {
        fn new(compression: Compression) -> Result<Self>;
        fn open(path: &Path) -> Result<Self>;
        fn capture(&mut self, path: &Path) -> Result<()>;
        fn write(&mut self, path: &Path) -> Result<()>;
        fn verify(&mut self) -> Result<()>;
        fn extract(&mut self, path: &Path) -> Result<()>;
    }

    impl Archive for Wim {
        fn new(compression: Compression) -> Result<Self> {
            Ok(Wim::new(compression)?)
        }
        fn open(path: &Path) -> Result<Self> {
            Ok(Wim::open(path, OpenOptions::default())?)
        }
        fn capture(&mut self, path: &Path) -> Result<()> {
            Ok(self.capture_image(path)?)
        }
        fn write(&mut self, path: &Path) -> Result<()> {
            Ok(Wim::write(self, path)?)
        }
        fn verify(&mut self) -> Result<()> {
            Ok(Wim::verify(self)?)
        }
        fn extract(&mut self, path: &Path) -> Result<()> {
            Ok(self.extract_image(ImageIndex::try_from(1)?, path)?)
        }
    }

    struct CArchive(NonNull<ffi::WimHandle>);
    impl Drop for CArchive {
        fn drop(&mut self) {
            // SAFETY: This owner releases its live handle exactly once.
            unsafe { ffi::wimlib_free(self.0.as_ptr()) };
        }
    }
    fn check(code: i32) -> Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(wim::Error::Engine(code).into())
        }
    }
    fn text(path: &Path) -> Result<CString> {
        Ok(CString::new(path.as_os_str().as_encoded_bytes())?)
    }
    impl Archive for CArchive {
        fn new(compression: Compression) -> Result<Self> {
            let mut handle = std::ptr::null_mut();
            // SAFETY: Supported codec and writable output storage.
            check(unsafe { ffi::wimlib_create_new_wim(compression.as_i32(), &mut handle) })?;
            Ok(Self(NonNull::new(handle).ok_or(wim::Error::MissingHandle)?))
        }
        fn open(path: &Path) -> Result<Self> {
            let path = text(path)?;
            let mut handle = std::ptr::null_mut();
            // SAFETY: Terminated pathname and writable output outlive the call.
            check(unsafe { ffi::wimlib_open_wim(path.as_ptr(), 0, &mut handle) })?;
            Ok(Self(NonNull::new(handle).ok_or(wim::Error::MissingHandle)?))
        }
        fn capture(&mut self, path: &Path) -> Result<()> {
            let path = text(path)?;
            // SAFETY: Exclusive live handle and terminated source; default name/config.
            check(unsafe {
                ffi::wimlib_add_image(
                    self.0.as_ptr(),
                    path.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                )
            })
        }
        fn write(&mut self, path: &Path) -> Result<()> {
            let path = text(path)?;
            // SAFETY: Live handle and terminated path, all images with integrity.
            check(unsafe { ffi::wimlib_write(self.0.as_ptr(), path.as_ptr(), -1, 1, 0) })
        }
        fn verify(&mut self) -> Result<()> {
            // SAFETY: This owner exclusively borrows the live handle.
            check(unsafe { ffi::wimlib_verify_wim(self.0.as_ptr(), 0) })
        }
        fn extract(&mut self, path: &Path) -> Result<()> {
            let path = text(path)?;
            // SAFETY: Live handle, image one, terminated target and default flags.
            check(unsafe { ffi::wimlib_extract_image(self.0.as_ptr(), 1, path.as_ptr(), 0) })
        }
    }

    fn run<A: Archive>(
        action: &str,
        source: &Path,
        target: &Path,
        codec: Compression,
    ) -> Result<()> {
        // Keep automatic global initialization outside every measured interval.
        drop(A::new(Compression::None)?);
        let mut times = [0.0; 5];
        match action {
            "write" => {
                let mut archive = A::new(codec)?;
                let start = Instant::now();
                archive.capture(black_box(source))?;
                times[0] = start.elapsed().as_secs_f64();
                let start = Instant::now();
                archive.write(black_box(target))?;
                times[1] = start.elapsed().as_secs_f64();
            }
            "read" => {
                let start = Instant::now();
                let mut archive = A::open(black_box(source))?;
                times[2] = start.elapsed().as_secs_f64();
                let start = Instant::now();
                archive.verify()?;
                times[3] = start.elapsed().as_secs_f64();
                let start = Instant::now();
                archive.extract(black_box(target))?;
                times[4] = start.elapsed().as_secs_f64();
            }
            _ => return Err("action must be write or read".into()),
        }
        let status = std::fs::read_to_string("/proc/self/status")?;
        let peak = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .and_then(|line| line.split_whitespace().next())
            .ok_or("missing VmHWM")?
            .parse::<u64>()?;
        println!(
            "{{\"capture_s\":{:.9},\"write_s\":{:.9},\"open_s\":{:.9},\"verify_s\":{:.9},\"apply_s\":{:.9},\"peak_rss_kib\":{peak}}}",
            times[0], times[1], times[2], times[3], times[4]
        );
        Ok(())
    }

    pub fn main() -> Result<()> {
        let args: Vec<_> = std::env::args_os().collect();
        if args.len() != 6 {
            return Err(
                "usage: benchmark_apis rust|c write|read SOURCE TARGET CODEC_NUMBER".into(),
            );
        }
        let api = args[1].to_str().ok_or("invalid API")?;
        let action = args[2].to_str().ok_or("invalid action")?;
        let codec = Compression::from_i32(args[5].to_str().ok_or("invalid codec")?.parse()?)?;
        match api {
            "rust" => run::<Wim>(action, Path::new(&args[3]), Path::new(&args[4]), codec),
            "c" => run::<CArchive>(action, Path::new(&args[3]), Path::new(&args[4]), codec),
            _ => Err("API must be rust or c".into()),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    return linux::main();
    #[cfg(not(target_os = "linux"))]
    Err("this benchmark requires Linux /proc RSS measurements".into())
}
