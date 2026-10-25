use eigentrail_core::{
    resources::memory::{self, Managed, Operation},
    serialization::input::{MAX_INPUT_BYTES, RequestKind, inspect_request},
    *,
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
};

fn read_bounded(mut input: impl Read, bytes: usize, remaining: usize) -> Result<Vec<u8>> {
    if bytes > remaining {
        return Err(Error::Resource(
            "serialized inputs exceed the shared input-copy budget".into(),
        ));
    }
    let mut out = memory::capacity(bytes)?;
    if out.capacity() > remaining {
        return Err(Error::Resource(
            "serialized input capacity exceeds budget".into(),
        ));
    }
    out.resize(bytes, 0);
    input.read_exact(&mut out).map_err(|e| {
        Error::Invalid(format!(
            "serialized input changed or could not be read: {e}"
        ))
    })?;
    let mut extra = [0];
    if input
        .read(&mut extra)
        .map_err(|e| Error::Invalid(e.to_string()))?
        != 0
    {
        return Err(Error::Resource(
            "serialized input grew while reading; retry a stable file".into(),
        ));
    }
    Ok(out)
}

fn input_file(path: &str) -> Result<(fs::File, usize)> {
    let file = fs::File::open(path).map_err(|e| Error::Invalid(e.to_string()))?;
    let metadata = file.metadata().map_err(|e| Error::Invalid(e.to_string()))?;
    if !metadata.is_file() {
        return Err(Error::Invalid(
            "regular serialized input file required".into(),
        ));
    }
    let bytes = usize::try_from(metadata.len())
        .map_err(|_| Error::Resource("serialized file size overflow".into()))?;
    if bytes > MAX_INPUT_BYTES {
        return Err(Error::Resource(
            "serialized file exceeds input ceiling".into(),
        ));
    }
    Ok((file, bytes))
}

fn read_request(
    path: &str,
    kind: RequestKind,
    cancel: &resources::Cancellation,
) -> Result<(Operation, Managed<Vec<u8>>, usize)> {
    let (mut file, _) = input_file(path)?;
    let header = inspect_request(&mut file, kind, cancel)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let current_bytes = usize::try_from(
        file.metadata()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .len(),
    )
    .map_err(|_| Error::Resource("serialized file size overflow".into()))?;
    if current_bytes != header.bytes {
        return Err(Error::Invalid("request changed during preflight".into()));
    }
    let limit = header.memory_bytes.min(MAX_INPUT_BYTES);
    let operation = Operation::new(header.memory_bytes)?;
    let bytes = operation.own(|| read_bounded(file, current_bytes, limit))?;
    let checked = inspect_request(bytes.as_slice(), kind, cancel)?;
    if checked.memory_bytes != header.memory_bytes || checked.bytes != bytes.len() {
        return Err(Error::Invalid(
            "request budget changed during input copying".into(),
        ));
    }
    let remaining = limit
        .checked_sub(bytes.capacity())
        .ok_or_else(|| Error::Resource("request capacity exceeds input-copy budget".into()))?;
    Ok((operation, bytes, remaining))
}

fn read(path: &str, remaining: &mut usize, operation: &Operation) -> Result<Managed<Vec<u8>>> {
    let (file, bytes) = input_file(path)?;
    let owned = operation.own(|| read_bounded(file, bytes, *remaining))?;
    *remaining = remaining
        .checked_sub(owned.capacity())
        .ok_or_else(|| Error::Resource("serialized input capacity exceeds budget".into()))?;
    Ok(owned)
}
fn execute() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let usage = "usage: eigentrail-check track|check|analyze-histories|check-histories REQUEST REPORT, or analyze-jumps|check-jumps|analyze-common-frame|check-common-frame REQUEST MODEL REPORT, or analyze-crossings|check-crossings REQUEST WINDOW REPORT, or analyze-transport|check-transport REQUEST REPORT";
    if args.len() == 2 {
        match args[1].as_str() {
            "-h" | "--help" => {
                println!(
                    "EigenTrail / TRACE\n{usage}\n\nAnalysis commands create a new report; existing output files are refused.\nCheck commands validate the supplied request/report locally without network access.\nConditional sampling and path assumptions remain explicit.\n\n  -h, --help       Show commands\n  -V, --version    Show package version"
                );
                return Ok(());
            }
            "-V" | "--version" => {
                println!("eigentrail-check {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => {}
        }
    }
    let arity = match args.get(1).map(String::as_str) {
        Some(
            "track" | "check" | "analyze-histories" | "check-histories" | "analyze-transport"
            | "check-transport",
        ) => 4,
        Some(
            "analyze-jumps"
            | "check-jumps"
            | "analyze-common-frame"
            | "check-common-frame"
            | "analyze-crossings"
            | "check-crossings",
        ) => 5,
        _ => return Err(Error::Invalid(usage.into())),
    };
    if args.len() != arity {
        return Err(Error::Invalid(usage.into()));
    }
    let histories =
        matches!(args[1].as_str(), "analyze-histories" | "check-histories") && args.len() == 4;
    let transport =
        matches!(args[1].as_str(), "analyze-transport" | "check-transport") && args.len() == 4;
    let cancel = resources::Cancellation::default();
    let kind = if transport {
        RequestKind::Transport
    } else if histories {
        RequestKind::History
    } else {
        RequestKind::Trace
    };
    let (operation, request_bytes, mut remaining) = read_request(&args[2], kind, &cancel)?;
    let _control = operation.external(64 * 1024)?;
    operation.run(|| {
    if transport {
        let request: transport_workflow::TransportRequest = serialization::decode_in(&request_bytes, &cancel)?;
        drop(request_bytes);
        if args[1] == "analyze-transport" {
            let assessment = transport_workflow::analyze(&request, &cancel)?;
            eigentrail_check::transport::check(&request, &assessment, &cancel)?;
            write_new(&args[3], &serialization::encode_with_cancel(&assessment, &cancel)?)?;
        } else {
            let assessment: transport_workflow::TransportAssessment =
                serialization::decode_in(&read(&args[3], &mut remaining, &operation)?, &cancel)?;
            eigentrail_check::transport::check(&request, &assessment, &cancel)?;
        }
        println!("checked initialized exact-polar geometry; covariance transport requires the separate history bridge");
        return Ok(());
    }
    if histories {
        let request: families::HistoryRequest = serialization::decode_in(&request_bytes, &cancel)?;
        drop(request_bytes);
        if args[1] == "analyze-histories" {
            let report = families::analyze(&request, &cancel)?;
            eigentrail_check::families::check(&request, &report, &cancel)?;
            write_new(&args[3], &serialization::encode(&report)?)?;
        } else {
            let report: families::HistoryReport =
                serialization::decode_in(&read(&args[3], &mut remaining, &operation)?, &cancel)?;
            eigentrail_check::families::check(&request, &report, &cancel)?;
        }
        println!(
            "checked symbolic whole-history family; model assumptions and resource/platform qualification remain explicit"
        );
        return Ok(());
    }
    let request: TraceRequest = serialization::decode_in(&request_bytes, &cancel)?;
    drop(request_bytes);
    match args[1].as_str() {
        "track" if args.len() == 4 => {
            let report = track(&request)?;
            eigentrail_check::check(&request, &report)?;
            let bytes = serialization::encode(&report)?;
            write_new(&args[3], &bytes)?;
            println!(
                "completed numerical check; calibration remains conditional on declared assumptions; platform/resource qualification pending"
            );
        }
        "check" if args.len() == 4 => {
            let report: TraceReport = serialization::decode_in(&read(&args[3], &mut remaining, &operation)?, &cancel)?;
            eigentrail_check::check(&request, &report)?;
            println!("checked");
        }
        "analyze-jumps" | "check-jumps" if args.len() == 5 => {
            let model: jumps::JumpModel = serialization::decode_in(&read(&args[3], &mut remaining, &operation)?, &cancel)?;
            if args[1] == "analyze-jumps" {
                let report = jumps::analyze_with_cancel(
                    &request,
                    &model,
                    &resources::Cancellation::default(),
                )?;
                eigentrail_check::jumps::check(&request, &model, &report)?;
                write_new(&args[4], &serialization::encode(&report)?)?;
            } else {
                let report: jumps::JumpReport =
                    serialization::decode_in(&read(&args[4], &mut remaining, &operation)?, &cancel)?;
                eigentrail_check::jumps::check(&request, &model, &report)?;
            }
            println!(
                "checked jump family; calibration and path assumptions remain conditional; platform/resource qualification pending"
            );
        }
        "analyze-crossings" | "check-crossings" if args.len() == 5 => {
            let window: crossings::CrossingWindow =
                serialization::decode_in(&read(&args[3], &mut remaining, &operation)?, &cancel)?;
            if args[1] == "analyze-crossings" {
                let assessment = crossings::analyze(&request, &window, &cancel)?;
                eigentrail_check::crossings::check(&request, &window, &assessment, &cancel)?;
                write_new(&args[4], &serialization::encode_with_cancel(&assessment, &cancel)?)?;
            } else {
                let assessment: crossings::CrossingAssessment =
                    serialization::decode_in(&read(&args[4], &mut remaining, &operation)?, &cancel)?;
                eigentrail_check::crossings::check(&request, &window, &assessment, &cancel)?;
            }
            println!("checked conditional crossing assessment; exact collision and regularity remain declared premises");
        }
        "analyze-common-frame" | "check-common-frame" if args.len() == 5 => {
            let model: common_frame::CommonFrameModel =
                serialization::decode_in(&read(&args[3], &mut remaining, &operation)?, &cancel)?;
            let cancel = resources::Cancellation::default();
            if args[1] == "analyze-common-frame" {
                let report = common_frame::analyze(&request, &model, &cancel)?;
                eigentrail_check::common_frame::check(&request, &model, &report, &cancel)?;
                write_new(&args[4], &serialization::encode(&report)?)?;
            } else {
                let report: common_frame::CommonFrameReport =
                    serialization::decode_in(&read(&args[4], &mut remaining, &operation)?, &cancel)?;
                eigentrail_check::common_frame::check(&request, &model, &report, &cancel)?;
            }
            println!(
                "checked persistent axes under supplied common-frame premise; resource/platform qualification pending"
            );
        }
        _ => return Err(Error::Invalid(usage.into())),
    }
    Ok(())
    })
}
fn write_new(path: &str, bytes: &[u8]) -> Result<()> {
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    output
        .write_all(bytes)
        .map_err(|e| Error::Invalid(e.to_string()))
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reads_reject_oversize_shrink_and_growth() {
        assert_eq!(read_bounded(&b"abcd"[..], 4, 4).unwrap(), b"abcd");
        assert!(matches!(
            read_bounded(&b"abcd"[..], 4, 3),
            Err(Error::Resource(_))
        ));
        assert!(matches!(
            read_bounded(&b"abc"[..], 4, 4),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            read_bounded(&b"abcde"[..], 4, 4),
            Err(Error::Resource(_))
        ));
    }

    #[test]
    fn input_copies_share_capacity_budget() {
        let mut remaining = 5usize;
        let request = read_bounded(&b"abc"[..], 3, remaining).unwrap();
        remaining -= request.capacity();
        assert!(matches!(
            read_bounded(&b"def"[..], 3, remaining),
            Err(Error::Resource(_))
        ));
        let companion = read_bounded(&b"de"[..], 2, remaining).unwrap();
        remaining -= companion.capacity();
        assert_eq!(remaining, 0);
        assert!(matches!(
            read_bounded(&b"f"[..], 1, remaining),
            Err(Error::Resource(_))
        ));
    }
}
