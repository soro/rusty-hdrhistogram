use clap::{Args, Parser, Subcommand};
use hdrhistogram::encoding::{
    histogram_log_base_time_line, histogram_log_legend_line, histogram_log_start_time_line, write_histogram_log_report,
    HistogramLogMovingWindowConfig, HistogramLogReportConfig, HistogramLogScannedRecord, HistogramLogScanner, HistogramLogTagFilter,
    DEFAULT_LOG_MAX_VALUE_UNIT_RATIO,
};
use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

type CliResult<T> = Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(name = "hdrhistogram")]
#[command(about = "Inspect and process HdrHistogram logs")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Log(LogCommand),
}

#[derive(Args)]
struct LogCommand {
    #[command(subcommand)]
    command: LogSubcommand,
}

#[derive(Subcommand)]
enum LogSubcommand {
    /// Generate interval and percentile reports from a histogram log.
    Process(ProcessArgs),
    /// List tags present in a histogram log without decoding payloads.
    Tags(InputArgs),
    /// Summarize interval metadata without decoding payloads.
    Summary(SummaryArgs),
    /// Print interval metadata without decoding payloads.
    Inspect(InspectArgs),
    /// Emit a filtered histogram log without decoding payloads.
    Filter(FilterArgs),
}

#[derive(Args)]
struct InputArgs {
    #[arg(short = 'i', long = "input", value_name = "FILE")]
    input: Option<PathBuf>,
}

#[derive(Args)]
struct ProcessArgs {
    #[arg(short = 'i', long = "input", value_name = "FILE")]
    input: Option<PathBuf>,
    #[arg(short = 'o', long = "output", value_name = "PREFIX")]
    output: Option<PathBuf>,
    #[arg(long = "csv")]
    csv: bool,
    #[arg(long = "list-tags")]
    list_tags: bool,
    #[arg(long = "java-list-tags-output", hide = true)]
    java_list_tags_output: bool,
    #[arg(long = "verbose", hide = true)]
    verbose: bool,
    #[arg(long = "tag", value_name = "TAG")]
    tag: Option<String>,
    #[arg(long = "all-tags")]
    all_tags: bool,
    #[arg(long = "start", default_value_t = 0.0)]
    start: f64,
    #[arg(long = "end")]
    end: Option<f64>,
    #[arg(long = "output-value-unit-ratio", alias = "outputValueUnitRatio", default_value_t = DEFAULT_LOG_MAX_VALUE_UNIT_RATIO)]
    output_value_unit_ratio: f64,
    #[arg(
        long = "percentiles-output-ticks-per-half",
        alias = "percentilesOutputTicksPerHalf",
        default_value_t = 5
    )]
    percentiles_output_ticks_per_half: u32,
    #[arg(
        long = "correct-log-with-known-coordinated-omission",
        alias = "correctLogWithKnownCoordinatedOmission",
        default_value_t = 0.0
    )]
    coordinated_omission_interval: f64,
    #[arg(long = "moving-window-percentile", alias = "mwp", value_name = "PERCENTILE")]
    moving_window_percentile: Option<f64>,
    #[arg(long = "moving-window-length-ms", alias = "mwpl", value_name = "MILLIS")]
    moving_window_length_ms: Option<u64>,
}

#[derive(Args)]
struct SummaryArgs {
    #[command(flatten)]
    input: InputArgs,
    #[command(flatten)]
    filter: ModernFilterArgs,
    #[arg(long = "json")]
    json: bool,
}

#[derive(Args)]
struct InspectArgs {
    #[command(flatten)]
    input: InputArgs,
    #[command(flatten)]
    filter: ModernFilterArgs,
    #[arg(long = "csv")]
    csv: bool,
    #[arg(long = "json")]
    json: bool,
}

#[derive(Args)]
struct FilterArgs {
    #[command(flatten)]
    input: InputArgs,
    #[command(flatten)]
    filter: ModernFilterArgs,
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct ModernFilterArgs {
    #[arg(long = "tag", value_name = "TAG")]
    tag: Option<String>,
    #[arg(long = "untagged")]
    untagged: bool,
    #[arg(long = "all-tags")]
    all_tags: bool,
    #[arg(long = "start", default_value_t = 0.0)]
    start: f64,
    #[arg(long = "end")]
    end: Option<f64>,
}

fn main() {
    let raw_args = env::args_os().collect::<Vec<_>>();
    let verbose = verbose_requested(&raw_args);
    if let Err(err) = run(Cli::parse_from(normalize_java_compatible_args(raw_args))) {
        if verbose {
            eprintln!("error: {:?}", err);
        } else {
            eprintln!("error: {}", err);
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> CliResult<()> {
    match cli.command {
        Command::Log(command) => match command.command {
            LogSubcommand::Process(args) => process_log(args),
            LogSubcommand::Tags(args) => list_tags(args),
            LogSubcommand::Summary(args) => summarize(args),
            LogSubcommand::Inspect(args) => inspect(args),
            LogSubcommand::Filter(args) => filter(args),
        },
    }
}

fn process_log(args: ProcessArgs) -> CliResult<()> {
    let reader = open_input(args.input.as_deref())?;
    if args.list_tags {
        let stdout = io::stdout();
        let style = if args.java_list_tags_output {
            TagsOutputStyle::Java
        } else {
            TagsOutputStyle::Plain
        };
        return write_tags(reader, &mut stdout.lock(), style);
    }
    let _verbose = args.verbose;
    let range_end_time_sec = checked_range_end(args.start, args.end)?;
    let output_prefix = args.output.as_deref().map(resolve_output_prefix).transpose()?;
    let output_to_files = output_prefix.is_some();

    let moving_window = match (args.moving_window_percentile, args.moving_window_length_ms) {
        (Some(percentile), Some(length_ms)) => Some(HistogramLogMovingWindowConfig {
            percentile_to_report: percentile,
            length_sec: length_ms as f64 / 1000.0,
        }),
        (Some(percentile), None) => Some(HistogramLogMovingWindowConfig {
            percentile_to_report: percentile,
            length_sec: 60.0,
        }),
        (None, Some(length_ms)) => Some(HistogramLogMovingWindowConfig {
            percentile_to_report: 99.0,
            length_sec: length_ms as f64 / 1000.0,
        }),
        (None, None) => None,
    };
    let config = HistogramLogReportConfig {
        tag_filter: java_process_tag_filter(args.tag, args.all_tags)?,
        range_start_time_sec: args.start,
        range_end_time_sec,
        output_value_unit_ratio: args.output_value_unit_ratio,
        percentile_ticks_per_half_distance: args.percentiles_output_ticks_per_half,
        csv: args.csv,
        moving_window: if output_to_files { moving_window } else { None },
        expected_interval_for_coordinated_omission_correction: args.coordinated_omission_interval,
        processor_time_range_headers: output_to_files,
        processor_start_time_header: true,
    };

    if let Some(output_prefix) = output_prefix {
        let mut interval_log = BufWriter::new(File::create(&output_prefix)?);
        let mut percentile_distribution = BufWriter::new(File::create(suffixed_path(&output_prefix, "hgrm"))?);
        if config.moving_window.is_some() {
            let mut moving_window_log = BufWriter::new(File::create(suffixed_path(&output_prefix, "mwp"))?);
            write_histogram_log_report(
                reader,
                &config,
                &mut interval_log,
                &mut percentile_distribution,
                Some(&mut moving_window_log),
            )?;
        } else {
            write_histogram_log_report(
                reader,
                &config,
                &mut interval_log,
                &mut percentile_distribution,
                None::<&mut io::Sink>,
            )?;
        }
        return Ok(());
    }

    let mut interval_sink = io::sink();
    let stdout = io::stdout();
    let mut percentile_distribution = stdout.lock();
    write_histogram_log_report(
        reader,
        &config,
        &mut interval_sink,
        &mut percentile_distribution,
        None::<&mut io::Sink>,
    )?;
    Ok(())
}

fn list_tags(args: InputArgs) -> CliResult<()> {
    let reader = open_input(args.input.as_deref())?;
    let stdout = io::stdout();
    write_tags(reader, &mut stdout.lock(), TagsOutputStyle::Plain)
}

fn summarize(args: SummaryArgs) -> CliResult<()> {
    let reader = open_input(args.input.input.as_deref())?;
    let filter = modern_tag_filter(&args.filter)?;
    let end = checked_range_end(args.filter.start, args.filter.end)?;
    let mut scanner = HistogramLogScanner::new(reader);
    let mut tags = BTreeSet::new();
    let mut total_intervals = 0_usize;
    let mut matched_intervals = 0_usize;

    while let Some(interval) = scanner.next_interval()? {
        total_intervals += 1;
        tags.insert(interval.tag.clone());
        if interval_matches(&interval, &filter, args.filter.start, end) {
            matched_intervals += 1;
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if args.json {
        write!(
            out,
            "{{\"total_intervals\":{},\"matched_intervals\":{},\"start_time_sec\":{:.3},\"base_time_sec\":{:.3},\"tags\":",
            total_intervals,
            matched_intervals,
            scanner.start_time_sec(),
            scanner.base_time_sec()
        )?;
        write_json_tags(&tags, &mut out)?;
        writeln!(out, "}}")?;
    } else {
        writeln!(out, "total_intervals: {}", total_intervals)?;
        writeln!(out, "matched_intervals: {}", matched_intervals)?;
        writeln!(out, "start_time_sec: {:.3}", scanner.start_time_sec())?;
        writeln!(out, "base_time_sec: {:.3}", scanner.base_time_sec())?;
        writeln!(out, "tags:")?;
        write_tag_set(&tags, &mut out, TagsOutputStyle::Plain)?;
    }
    Ok(())
}

fn inspect(args: InspectArgs) -> CliResult<()> {
    if args.csv && args.json {
        return Err(invalid_input("--csv and --json are mutually exclusive"));
    }
    let reader = open_input(args.input.input.as_deref())?;
    let filter = modern_tag_filter(&args.filter)?;
    let end = checked_range_end(args.filter.start, args.filter.end)?;
    let mut scanner = HistogramLogScanner::new(reader);
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.json {
        write!(out, "[")?;
    } else if args.csv {
        writeln!(
            out,
            "tag,start_timestamp_sec,interval_length_sec,max_value,absolute_start_time_sec,absolute_end_time_sec,relative_start_time_sec,relative_end_time_sec"
        )?;
    } else {
        writeln!(
            out,
            "tag\tstart_timestamp_sec\tinterval_length_sec\tmax_value\tabsolute_start_time_sec\tabsolute_end_time_sec\trelative_start_time_sec\trelative_end_time_sec"
        )?;
    }

    let mut first_json_interval = true;
    while let Some(interval) = scanner.next_interval()? {
        if !interval_matches(&interval, &filter, args.filter.start, end) {
            continue;
        }
        let tag = interval.tag.as_deref().unwrap_or("");
        if args.json {
            if !first_json_interval {
                write!(out, ",")?;
            }
            first_json_interval = false;
            write_json_interval(&interval, &mut out)?;
        } else if args.csv {
            writeln!(
                out,
                "{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
                tag,
                interval.start_timestamp_sec,
                interval.interval_length_sec,
                interval.max_value,
                interval.absolute_start_time_sec,
                interval.absolute_end_time_sec,
                interval.relative_start_time_sec,
                interval.relative_end_time_sec,
            )?;
        } else {
            writeln!(
                out,
                "{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
                tag,
                interval.start_timestamp_sec,
                interval.interval_length_sec,
                interval.max_value,
                interval.absolute_start_time_sec,
                interval.absolute_end_time_sec,
                interval.relative_start_time_sec,
                interval.relative_end_time_sec,
            )?;
        }
    }
    if args.json {
        writeln!(out, "]")?;
    }
    Ok(())
}

fn filter(args: FilterArgs) -> CliResult<()> {
    let reader = open_input(args.input.input.as_deref())?;
    let filter = modern_tag_filter(&args.filter)?;
    let end = checked_range_end(args.filter.start, args.filter.end)?;
    let mut scanner = HistogramLogScanner::new(reader);

    let stdout = io::stdout();
    let mut stdout_lock;
    let mut file_output;
    let out: &mut dyn Write = if let Some(path) = args.output.as_deref() {
        file_output = BufWriter::new(File::create(path)?);
        &mut file_output
    } else {
        stdout_lock = stdout.lock();
        &mut stdout_lock
    };

    while let Some(record) = scanner.next_record()? {
        match record {
            HistogramLogScannedRecord::StartTime(value) => out.write_all(histogram_log_start_time_line(value).as_bytes())?,
            HistogramLogScannedRecord::BaseTime(value) => out.write_all(histogram_log_base_time_line(value).as_bytes())?,
            HistogramLogScannedRecord::Comment(comment) => writeln!(out, "#{}", comment)?,
            HistogramLogScannedRecord::Legend => out.write_all(histogram_log_legend_line().as_bytes())?,
            HistogramLogScannedRecord::Interval(interval) => {
                if interval_matches(&interval, &filter, args.filter.start, end) {
                    out.write_all(interval.log_line().as_bytes())?;
                }
            }
        }
    }
    Ok(())
}

fn open_input(path: Option<&Path>) -> CliResult<Box<dyn BufRead>> {
    match path {
        Some(path) => Ok(Box::new(BufReader::new(File::open(path)?))),
        None => Ok(Box::new(BufReader::new(io::stdin()))),
    }
}

#[derive(Clone, Copy)]
enum TagsOutputStyle {
    Plain,
    Java,
}

fn write_tags<R: BufRead, W: Write>(reader: R, out: &mut W, style: TagsOutputStyle) -> CliResult<()> {
    let mut scanner = HistogramLogScanner::new(reader);
    let mut tags = BTreeSet::new();
    while let Some(interval) = scanner.next_interval()? {
        tags.insert(interval.tag);
    }
    write_tag_set(&tags, out, style)?;
    Ok(())
}

fn write_tag_set<W: Write>(tags: &BTreeSet<Option<String>>, out: &mut W, style: TagsOutputStyle) -> CliResult<()> {
    if matches!(style, TagsOutputStyle::Java) {
        writeln!(out, "Tags found in input file:")?;
    }
    for tag in tags {
        match tag {
            Some(tag) => writeln!(out, "{}", tag)?,
            None => match style {
                TagsOutputStyle::Plain => writeln!(out, "[NO TAG]")?,
                TagsOutputStyle::Java => writeln!(out, "[NO TAG (default)]")?,
            },
        }
    }
    Ok(())
}

fn write_json_tags<W: Write>(tags: &BTreeSet<Option<String>>, out: &mut W) -> io::Result<()> {
    write!(out, "[")?;
    for (index, tag) in tags.iter().enumerate() {
        if index > 0 {
            write!(out, ",")?;
        }
        match tag {
            Some(tag) => write_json_string(tag, out)?,
            None => write!(out, "null")?,
        }
    }
    write!(out, "]")
}

fn write_json_interval<W: Write>(interval: &hdrhistogram::encoding::HistogramLogScannedInterval, out: &mut W) -> io::Result<()> {
    write!(out, "{{\"tag\":")?;
    match interval.tag.as_deref() {
        Some(tag) => write_json_string(tag, out)?,
        None => write!(out, "null")?,
    }
    write!(
        out,
        ",\"start_timestamp_sec\":{:.3},\"interval_length_sec\":{:.3},\"max_value\":{:.3},\"absolute_start_time_sec\":{:.3},\"absolute_end_time_sec\":{:.3},\"relative_start_time_sec\":{:.3},\"relative_end_time_sec\":{:.3}}}",
        interval.start_timestamp_sec,
        interval.interval_length_sec,
        interval.max_value,
        interval.absolute_start_time_sec,
        interval.absolute_end_time_sec,
        interval.relative_start_time_sec,
        interval.relative_end_time_sec
    )
}

fn write_json_string<W: Write>(value: &str, out: &mut W) -> io::Result<()> {
    write!(out, "\"")?;
    for ch in value.chars() {
        match ch {
            '"' => write!(out, "\\\"")?,
            '\\' => write!(out, "\\\\")?,
            '\n' => write!(out, "\\n")?,
            '\r' => write!(out, "\\r")?,
            '\t' => write!(out, "\\t")?,
            ch if ch.is_control() => write!(out, "\\u{:04x}", ch as u32)?,
            ch => write!(out, "{}", ch)?,
        }
    }
    write!(out, "\"")
}

fn normalize_java_compatible_args<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(program) = args.next() else {
        return Vec::new();
    };
    let rest = args.collect::<Vec<_>>();

    let mut normalized = Vec::with_capacity(rest.len() + 3);
    normalized.push(program);
    if should_use_java_process_mode(&rest) {
        normalized.push(OsString::from("log"));
        normalized.push(OsString::from("process"));
    }
    for arg in rest {
        push_normalized_java_arg(arg, &mut normalized);
    }
    normalized
}

fn should_use_java_process_mode(args: &[OsString]) -> bool {
    args.first().and_then(|arg| arg.to_str()).is_some_and(is_java_processor_option)
}

fn is_java_processor_option(arg: &str) -> bool {
    matches!(
        arg,
        "-csv"
            | "-v"
            | "-listtags"
            | "-alltags"
            | "-i"
            | "-tag"
            | "-mwp"
            | "-mwpl"
            | "-start"
            | "-end"
            | "-o"
            | "-percentilesOutputTicksPerHalf"
            | "-outputValueUnitRatio"
            | "-correctLogWithKnownCoordinatedOmission"
            | "-h"
    )
}

fn push_normalized_java_arg(arg: OsString, normalized: &mut Vec<OsString>) {
    let Some(arg_str) = arg.to_str() else {
        normalized.push(arg);
        return;
    };

    match arg_str {
        "-csv" => normalized.push(OsString::from("--csv")),
        "-v" => normalized.push(OsString::from("--verbose")),
        "-listtags" => {
            normalized.push(OsString::from("--list-tags"));
            normalized.push(OsString::from("--java-list-tags-output"));
        }
        "-alltags" => normalized.push(OsString::from("--all-tags")),
        "-tag" => normalized.push(OsString::from("--tag")),
        "-mwp" => normalized.push(OsString::from("--moving-window-percentile")),
        "-mwpl" => normalized.push(OsString::from("--moving-window-length-ms")),
        "-start" => normalized.push(OsString::from("--start")),
        "-end" => normalized.push(OsString::from("--end")),
        "-percentilesOutputTicksPerHalf" => normalized.push(OsString::from("--percentiles-output-ticks-per-half")),
        "-outputValueUnitRatio" => normalized.push(OsString::from("--output-value-unit-ratio")),
        "-correctLogWithKnownCoordinatedOmission" => {
            normalized.push(OsString::from("--correct-log-with-known-coordinated-omission"));
        }
        "-h" => normalized.push(OsString::from("--help")),
        _ => normalized.push(arg),
    }
}

fn verbose_requested(args: &[OsString]) -> bool {
    args.iter()
        .filter_map(|arg| arg.to_str())
        .any(|arg| arg == "-v" || arg == "--verbose")
}

fn java_process_tag_filter(tag: Option<String>, all_tags: bool) -> CliResult<HistogramLogTagFilter> {
    if all_tags && tag.is_some() {
        return Err(invalid_input("--all-tags cannot be combined with --tag"));
    }
    if all_tags {
        Ok(HistogramLogTagFilter::Any)
    } else if let Some(tag) = tag {
        Ok(HistogramLogTagFilter::Tag(tag))
    } else {
        Ok(HistogramLogTagFilter::Untagged)
    }
}

fn modern_tag_filter(args: &ModernFilterArgs) -> CliResult<HistogramLogTagFilter> {
    let selected = usize::from(args.tag.is_some()) + usize::from(args.untagged) + usize::from(args.all_tags);
    if selected > 1 {
        return Err(invalid_input("--tag, --untagged, and --all-tags are mutually exclusive"));
    }
    if let Some(tag) = args.tag.as_ref() {
        Ok(HistogramLogTagFilter::Tag(tag.clone()))
    } else if args.untagged {
        Ok(HistogramLogTagFilter::Untagged)
    } else {
        Ok(HistogramLogTagFilter::Any)
    }
}

fn interval_matches(
    interval: &hdrhistogram::encoding::HistogramLogScannedInterval,
    tag_filter: &HistogramLogTagFilter,
    start: f64,
    end: f64,
) -> bool {
    interval.relative_start_time_sec >= start
        && interval.relative_start_time_sec <= end
        && match tag_filter {
            HistogramLogTagFilter::Untagged => interval.tag.is_none(),
            HistogramLogTagFilter::Tag(tag) => interval.tag.as_deref() == Some(tag.as_str()),
            HistogramLogTagFilter::Any => true,
        }
}

fn checked_range_end(start: f64, end: Option<f64>) -> CliResult<f64> {
    let end = end.unwrap_or(f64::MAX);
    if !start.is_finite() || end.is_nan() {
        return Err(invalid_input("range start must be finite and range end must not be NaN"));
    }
    if end < start {
        return Err(invalid_input("range end must not be before range start"));
    }
    Ok(end)
}

fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), suffix))
}

fn resolve_output_prefix(path: &Path) -> CliResult<PathBuf> {
    let process_id = std::process::id().to_string();
    let date = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string();
    let replaced = path.to_string_lossy().replace("%pid", &process_id).replace("%date", &date);
    Ok(PathBuf::from(replaced))
}

fn invalid_input(message: &'static str) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message))
}
