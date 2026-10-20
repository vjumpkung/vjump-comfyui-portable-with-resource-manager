#![cfg(windows)]

//! The desktop binary reserves an 8 MiB main-thread stack (`build.rs`); test executables keep
//! the linker default. The PE header is read directly, so the test locks the size, not usage.

const EIGHT_MIB: u64 = 8 * 1024 * 1024;

#[test]
fn desktop_exe_reserves_an_eight_mib_main_stack() {
    let image = std::fs::read(env!("CARGO_BIN_EXE_vjumpkung-comfyui-desktop"))
        .expect("the desktop executable must be readable");
    let reserve = stack_reserve(&image).expect("the desktop executable must be a PE image");
    assert!(
        reserve >= EIGHT_MIB,
        "the desktop executable reserves {reserve} bytes of main-thread stack"
    );
}

#[test]
fn test_executables_keep_the_default_stack_reserve() {
    let image = std::fs::read(std::env::current_exe().expect("the test executable path"))
        .expect("the test executable must be readable");
    let reserve = stack_reserve(&image).expect("the test executable must be a PE image");
    assert!(
        reserve < EIGHT_MIB,
        "the test executable reserves {reserve} bytes of main-thread stack"
    );
}

/// Reads `SizeOfStackReserve` from the optional header of a PE image.
fn stack_reserve(image: &[u8]) -> Result<u64, String> {
    fn bytes<const N: usize>(image: &[u8], at: usize, what: &str) -> Result<[u8; N], String> {
        let end = at
            .checked_add(N)
            .ok_or_else(|| format!("{what} offset overflows"))?;
        image
            .get(at..end)
            .and_then(|slice| slice.try_into().ok())
            .ok_or_else(|| format!("{what} is outside the image"))
    }

    if bytes::<2>(image, 0, "the DOS signature")? != *b"MZ" {
        return Err("missing MZ signature".to_owned());
    }
    let pe = u32::from_le_bytes(bytes(image, 0x3C, "e_lfanew")?) as usize;
    if bytes::<4>(image, pe, "the PE signature")? != *b"PE\0\0" {
        return Err("missing PE signature".to_owned());
    }
    let optional_size = u16::from_le_bytes(bytes(image, pe + 20, "SizeOfOptionalHeader")?);
    if optional_size < 80 {
        return Err(format!("optional header is too small ({optional_size})"));
    }
    let optional = pe + 24;
    match u16::from_le_bytes(bytes(image, optional, "the optional header magic")?) {
        0x20B => Ok(u64::from_le_bytes(bytes(
            image,
            optional + 72,
            "SizeOfStackReserve",
        )?)),
        0x10B => Ok(u64::from(u32::from_le_bytes(bytes(
            image,
            optional + 72,
            "SizeOfStackReserve",
        )?))),
        magic => Err(format!("unknown optional header magic {magic:#x}")),
    }
}
