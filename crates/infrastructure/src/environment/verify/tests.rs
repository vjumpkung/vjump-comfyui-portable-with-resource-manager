use super::*;

fn expectations() -> VerifyExpectations {
    VerifyExpectations {
        torch_version_prefix: "2.14.1+cu130".into(),
        require_cuda: true,
        require_triton: true,
    }
}

#[test]
fn torch_must_match_the_profile_prefix_and_cuda_requirement() {
    let good = TorchReport {
        version: "2.14.1+cu130".into(),
        cuda_available: true,
    };
    assert!(check_torch(&good, &expectations()).is_ok());
    let wrong_build = TorchReport {
        version: "2.14.1+cpu".into(),
        cuda_available: true,
    };
    assert!(matches!(
        check_torch(&wrong_build, &expectations()),
        Err(ProvisionError::Failed(message)) if message.contains("2.14.1+cu130")
    ));
    let no_cuda = TorchReport {
        version: "2.14.1+cu130".into(),
        cuda_available: false,
    };
    assert!(check_torch(&no_cuda, &expectations()).is_err());
    let relaxed = VerifyExpectations {
        require_cuda: false,
        ..expectations()
    };
    assert!(check_torch(&no_cuda, &relaxed).is_ok());
}

#[test]
fn results_are_parsed_from_the_marker_line_amid_noise() {
    let stdout =
        "warning: something\nVJ-VERIFY:{\"version\":\"2.14.1+cu130\",\"cuda\":true}\nmore\n";
    let info: TorchInfo = parse(stdout).unwrap();
    assert_eq!(info.version, "2.14.1+cu130");
    assert!(info.cuda);
    assert!(parse::<TorchInfo>("nothing").is_err());
    assert!(parse::<TorchInfo>("VJ-VERIFY:{\"version\":1}").is_err());
    let final_line = "VJ-VERIFY:{\"python\":\"3.13.16\",\"torch\":\"2.14.1+cu130\",\"cuda\":true,\"triton\":\"3.8.0\",\"comfy\":\"C:/ComfyUI/comfy\",\"device\":\"RTX 3060\",\"cuda_ok\":true,\"accelerators\":{\"sageattention\":false,\"flash_attn\":false,\"llama_cpp\":true}}";
    let info: FinalInfo = parse(final_line).unwrap();
    assert_eq!(info.accelerators.len(), 3);
    assert_eq!(info.cuda_ok, Some(true));
}
