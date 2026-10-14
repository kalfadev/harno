# Code map {05cf1d}

The code in areas: the core harness (`crates/harno`, `crates/harno-log`) and the built-in tools (`tools/`); mdmap, the docs command, is a repository of its own with its own code map. Each area node says what the area owns, the rules it must keep, what else changes with it, and which tests cover it; its last block lists every item of its files with line spans (`harno run mdmap verify` keeps it current, the `areas` preset in `ci` fails when a source file belongs to no area). The code itself is not here: fetch the items a task needs with `harno run body -- <name>…` in the same call as `harno run mdmap show`.

## Commands: run {d73688}

Every command the agent runs (`run.rs`): splitting, the allowlist, presets, `--seq`/`--par`, timeouts; the output then goes through the filters.

Rules. A command is an argument vector, never a shell string: `&&`, `|`, `$VAR`, `;` are plain arguments; quotes group, a backslash escapes, an unquoted `*`, `?`, `[` is a glob against the project directory. `run.allow` matches word by word as a prefix, a word of the entry may be a glob; outside it the refusal offers `harno run --raw "<cmd>" --why "<reason>"`, and `--raw` needs `--why`, which is logged. An entry is a command or `{command: <words>, timeout: <seconds>}`: the longest entry that matches gives the timeout, else `run.timeout`, for a command typed by hand or in a preset; a verify line of mdmap has its own list. Presets skip `run.allow` on purpose: `run.allow` is what the agent may start by hand, a preset holds commands that run only as written there. A preset item may name another preset: its commands take the place of the item and run the way of the preset around them; a seq preset of several commands inside a par one, and a preset that includes itself, are refused. An item that names its own preset is the program of that name, so `mdmap: { seq: ["mdmap {args}"] }` runs the `mdmap` of PATH. `{args}` as a word of its own puts each argument in as one single-quoted word. Inside a word or a quoted string it takes exactly one argument, written as it is, of letters, digits and `_ . : / -` only, so an argument never reaches a shell, a quote or a glob, whatever program the preset starts; this is what lets a preset build a pattern the agent would otherwise type with `$` or `(`, which the harness asks about. `defaults: {<flag>: <value>}` of a preset go in front of the arguments of the caller flag by flag, unless the caller writes that flag (`-n 5`, `-n5`, `--max-count=5`): `log -- -n 100` runs `git log --oneline -n 100` and `log -- src/` keeps `-n 20`. An empty value is a flag alone, and a preset with defaults needs an `{args}` (`with_defaults`). The owner chose flag by flag, as snip does, over all or nothing, which would lose `-n 20` to a path (2026-09-23). Every item of `--seq`/`--par` is a unit: a command or a preset, which runs its own way; commands are numbered across the call. `--seq` stops at the first failure and exits with its code; `--par` runs all and exits with the first failure by index; several commands without a flag run in sequence, except after a lone preset: every word after its name is its argument, flags too, `--` first or not; an item of `--seq`/`--par` takes its arguments after `--`. stdout and stderr share one pipe, which keeps the order they were written in, unless a filter the command may get reads one of them (`reads`): then each has a pipe of its own, read in the order the chunks arrive (`exec`, **Filters**); a timeout stops the command.

The sandbox (`sandbox.rs`). `run.sandbox` holds `enabled`, `allow` and `deny`, and with it on every command of a call, `--raw` and a preset alike, runs inside it. Writing is closed: the project, the devices a program needs and the paths of `allow.write` are writable, everything else is refused. Reading is named: `allow.read` is `["*"]`, every path, until a project narrows it, and then that list, the project and the system directories a program starts from are the whole of what may be read. A path of `deny.read` beats every allow, the project included, so an allow inside a denied path is left out of the profile rather than written and overruled. Starting a program is open on purpose: `run.allow` is what says which command may start. A `~` is the home directory and every path is resolved before it becomes a rule, since a rule about /tmp says nothing about /private/tmp. macOS wraps the program in `sandbox-exec` with a profile written from the config, where the last rule that matches wins, so every deny is written after the allows. Linux builds a Landlock ruleset in the caller and applies it in the child, between the fork and the program; Landlock has no deny, so a narrowed read of either kind becomes an allow of the system directories, the project and `allow.read`, each one under a denied path dropped. The program is looked up in PATH first, so a command that is not there still answers 127 rather than failing inside the sandbox. Whether a sandbox can start at all is asked once per process (`unavailable`): on macOS by starting one, since an outer sandbox refuses a sandbox of its own and the refusal shows nowhere else; on Linux by the Landlock ABI of the kernel. When none can start nothing is wrapped, the commands run as they are, and `cli::main` ends every call of that process with the line `notice` writes: a note when macOS answers `sandbox_apply: Operation not permitted`, since an outer sandbox then holds the process and asks nothing of the reader, else a warning that nothing guards the commands. A filter script is outside all of it: it runs as the project wrote it.

Live output (`Live`). While a command runs, every whole line it writes goes to the `<n>.raw` of the call, so that `harno log raw <n>` reads a call that has not ended, and to the terminal when a human watches (`Env::watched`, a `Screen`: stdout when it is one, and no `--json`); when the call runs several commands a line comes under the header of its own, written again whenever another command wrote last. Once the commands ended the call closes it (`close`): what a process they left behind writes later reaches neither the file nor the terminal. What the agent reads does not change: a filter reads the whole output, so the output is filtered once the command ends. A watched call is neither filtered nor printed again: its outputs move into `raw`, `<n>.raw` keeps them, and the call ends with `blocks` of the rest, the headers and the summary. A timeout of `0` is none, in `run.timeout` and in an entry alike, a filter script included: the command runs until it ends or is stopped. That is how a server runs: the agent starts it in the background of its harness, `harno log ls` shows it `running`, and `harno log raw <n>` gives what it wrote so far, its address with it. A test meets a call while it runs through `Project::turns`, a command that goes step by step, waiting on lines of the raw file or on files the test writes, and `wait_for`; `harno_watched` gives the call a screen of its own and returns what reached it.

Stopping (`signals.rs`). Every command runs in a process group of its own (`own_group`), and the group of each command that runs is held while it runs (`hold`, `release`). A timeout stops the whole group (`kill_group`), so what the command started goes with it. SIGTERM, SIGINT and SIGHUP to harno go on to every group held; once the command ends, what is left of its group is killed too, a process that ignored the signal included, and the call ends as it ends when a command fails, its line written, exit 128 and the number of the signal (`code`). A signal that finds no command running ends harno as it would without the handler, and one ignored when harno started stays ignored, so `nohup` holds. SIGKILL reaches no handler: its commands run on, and the call is `died` in `log ls`. The tests start the binary, `crates/harno/tests/signals.rs`, since a signal to a test that runs harno in-process would stop the test.

When changing. The output format is asserted exactly in tests. A header of several commands (`blocks`) leaves out `ok` and a time under a second, as a call of one command does, since the footer counts what passed; the owner chose that (2026-09-23) over a short name in place of the command, which would take a call to read back. A preset feature: `unit`, `preset_args`, `expand`, `with_args` and `args_at` here, Guide: Use. Tests: tests/run.rs; preset arguments are in tests/filter.rs; signals in crates/harno/tests/signals.rs.

<!-- verify: outline crates/harno/src/run.rs crates/harno/src/sandbox.rs crates/harno/src/signals.rs crates/harno-test/tests/run.rs crates/harno-test/tests/sandbox.rs crates/harno/tests/signals.rs -->

```
crates/harno-test/tests/run.rs
  5-19  fn project() -> Project
  22-31  fn seq_runs_in_order_and_stops_at_the_first_failure()
  34-39  fn seq_that_passes()
  42-52  fn a_call_of_one_command_gives_back_its_output_alone()
  55-62  fn a_long_command_is_cut_in_a_header()
  65-71  fn a_block_that_passed_at_once_is_its_command_alone()
  74-87  fn par_runs_everything_and_reports_every_result()
  90-95  fn par_is_parallel()
  98-103  fn a_command_not_in_the_allowlist_is_refused_with_the_raw_form()
  106-118  fn the_words_of_one_allowed_command_given_apart_are_told_to_go_in_quotes()
  121-129  fn allowlist_is_a_word_prefix()
  132-137  fn nothing_runs_when_one_command_of_a_chain_is_refused()
  140-151  fn raw_runs_anything_and_logs_why()
  154-160  fn raw_needs_a_reason()
  163-173  fn presets_need_no_allow_entry()
  176-194  fn seq_and_par_take_presets()
  197-203  fn no_shell_interpretation()
  206-212  fn quotes_group_words_and_unquoted_globs_expand()
  215-230  fn a_script_path_is_a_command()
  233-240  fn several_commands_without_a_flag_run_in_sequence()
  243-248  fn par_json_lists_every_exit_code()
  252-257  fn stepwise() -> Project
  260-262  fn raw(n: u32) -> String
  265-277  fn a_timeout_of_zero_is_none()
  280-292  fn a_command_writes_into_the_raw_file_of_the_call_as_it_runs()
  295-308  fn several_commands_write_under_their_headers_as_they_take_turns()
  311-335  fn a_watched_call_shows_each_line_as_it_comes_unfiltered_and_not_again()
  338-353  fn what_a_process_left_behind_writes_after_the_call_reaches_nothing()
  356-363  fn a_timeout_stops_the_command()
  366-375  fn an_allow_entry_can_carry_its_own_timeout()
  378-384  fn a_missing_program()
  387-391  fn stdout_and_stderr_arrive_together()
  394-399  fn run_json()
  402-416  fn json_carries_the_warnings_as_a_field_so_the_output_stays_one_json_value()
  419-436  fn run_refuses_what_it_cannot_run_before_it_runs_anything()
  438-461  fn nested() -> Project
  464-477  fn a_preset_item_can_name_another_preset()
  480-488  fn a_nested_preset_runs_the_way_of_the_preset_around_it()
  491-498  fn an_item_that_names_its_own_preset_is_the_program_of_that_name()
crates/harno-test/tests/sandbox.rs
  8-12  fn project() -> Project
  15-17  fn warning() -> Option<String>
  20-25  fn an_outer_sandbox_is_a_note_and_no_sandbox_a_warning()
  28-39  fn the_profile_opens_the_project_and_the_written_paths_and_nothing_else()
  42-54  fn a_denied_path_beats_an_allow_inside_it()
  57-61  fn a_star_with_nothing_denied_leaves_reading_open()
  64-73  fn a_read_list_without_a_star_is_the_whole_of_what_may_be_read()
  76-80  fn a_command_writes_inside_the_project()
  83-98  fn a_command_does_not_write_outside_the_project()
  101-108  fn every_call_carries_the_warning_while_no_sandbox_runs()
  111-115  fn a_call_of_a_project_without_the_sandbox_says_nothing_about_it()
crates/harno/src/run.rs
  24-29  pub struct Word
  31-91  pub fn split(s: &str) -> R<Vec<Word>>
  95-125  pub fn argv(words: &[Word], dir: &Path) -> Vec<String>
  130-143  pub fn allowed<'a>(allow: &'a [Allow], words: &[Word]) -> Option<&'a Allow>
  147-151  fn one_command(config: &Config, items: &[String]) -> Option<Refusal>
  155-163  pub fn refusal_name(allow: &[Allow], words: &[Word], raw: &str) -> String
  168-171  pub fn timeout_of(config: &Config, command: &str) -> Duration
  174-189  pub struct Ran
  192-192  type Chunk = (bool, Vec<u8>);
  195-195  pub type Screen = Arc<Mutex<dyn Write + Send>>;
  202-202  pub struct Live(Arc<Mutex<Sink>>);
  204-210  struct Sink
  212-257  impl Live
  214-217    pub fn new(file: Option<File>, screen: Option<Screen>) -> Option<Live>
  221-223    pub fn watched(&self) -> bool
  225-227    fn several(&self, yes: bool)
  231-235    fn close(&self)
  237-256    fn write(&self, index: usize, command: &str, lines: &[u8])
  265-410  pub fn exec(command: &str, dir: &Path, timeout: Duration, apart: bool, sandbox: Option<&Sandbox>, live: Option<(&Live, usize)>) -> (i32, String, String, f64, Option<(String, String)>)
  415-429  fn which(program: &str) -> Option<PathBuf>
  431-433  fn secs(started: Instant) -> f64
  436-439  pub enum Mode
  441-453  pub struct RunArgs
  455-601  pub fn run(project: &Path, config: &Config, a: RunArgs, json_out: bool) -> R<Output>
  606-606  const HEADER: usize = 60;
  609-609  const SLOW: f64 = 1.0;
  616-624  fn headline(command: &str, max: usize) -> String
  628-634  fn shown(r: &Ran, raw: bool) -> &str
  644-680  fn blocks(ran: &[Ran], footer: &str, raw: bool, total: usize) -> String
  683-685  pub fn shown_per_command(text: &str, commands: usize) -> Vec<u64>
  690-723  pub fn text_per_command(text: &str, commands: usize) -> Vec<String>
  726-726  type Commands = Vec<(usize, String, Duration)>;
  730-733  struct Unit
  738-754  fn unit(config: &Config, item: &str, given: Option<&[String]>, n: &mut usize) -> R<Unit>
  762-794  pub fn preset_of(config: &Config, named: &[String], command: &str) -> Option<String>
  799-810  fn fits(template: &str, command: &str) -> bool
  814-821  fn preset_args(words: &[Word], given: Option<&[String]>) -> R<Vec<String>>
  829-866  fn expand(config: &Config, name: &str, args: &[String], outer: &mut Vec<String>, n: &mut usize) -> R<(Mode, Commands)>
  868-879  fn exec_one(index: usize, command: &str, dir: &Path, timeout: Duration, sandbox: Option<&Sandbox>, filters: &filter::Filters, live: Option<&Live>) -> Ran
  883-913  fn exec_unit(u: &Unit, dir: &Path, sandbox: Option<&Sandbox>, filters: &filter::Filters, live: Option<&Live>) -> Vec<Ran>
  919-931  fn filter_output(project: &Path, config: &Config, r: &mut Ran, timeout: Duration, call: Option<u64>) -> Option<String>
  938-948  fn with_args(command: &str, args: &[String]) -> R<String>
  951-959  fn inside(command: &str, args: &[String]) -> R<String>
  964-986  fn args_at(command: &str) -> Vec<(usize, bool)>
  991-1005  fn with_defaults(p: &Preset, args: &[String]) -> Vec<String>
  1010-1017  pub fn one_command_preset(config: &Config, words: &[Word]) -> Option<String>
  1021-1023  fn single_quote(s: &str) -> String
  1025-1027  pub fn quote_always(s: &str) -> String
crates/harno/src/sandbox.rs
  18-18  const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
  22-22  const DEVICES: &[&str] = &["/dev/null", "/dev/zero", "/dev/tty", "/dev/fd", "/dev/stdout", "/dev/stderr", "/dev/dtracehelper"];
  26-26  const SYSTEM: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/opt", "/proc", "/sys", "/dev", "/var", "/tmp", "/run"];
  32-44  pub struct Sandbox
  46-172  impl Sandbox
  51-58    pub fn of(config: &Config, project: &Path) -> Option<Sandbox>
  60-69    pub fn new(project: &Path, write: &[String], read: &[String], deny_read: &[String], home: &Path) -> Sandbox
  73-79    fn writable(&self) -> Vec<PathBuf>
  84-90    fn readable(&self) -> Vec<PathBuf>
  95-115    pub fn profile(&self) -> String
  120-127    pub fn command(&self, program: &str, args: &[String]) -> (String, Vec<String>)
  132-166    pub fn restrict(&self, cmd: &mut Command) -> Result<(), String>
  169-171    pub fn restrict(&self, _cmd: &mut Command) -> Result<(), String>
  176-179  pub fn unavailable() -> Option<&'static str>
  177-177    static WHY: OnceLock<Option<String>> = OnceLock::new();
  186-192  pub fn notice(why: &str) -> String
  197-206  fn probe() -> Option<String>
  212-215  fn probe() -> Option<String>
  218-220  fn probe() -> Option<String>
  224-226  fn line_of(text: &str) -> Option<String>
  229-235  fn expand(path: &str, home: &Path) -> PathBuf
  239-241  fn resolve(path: &Path) -> PathBuf
  244-246  fn lisp(path: &Path) -> String
crates/harno/src/signals.rs
  15-15  static GROUPS: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];
  19-19  static STOPPED: AtomicI32 = AtomicI32::new(0);
  22-42  pub fn own_group(cmd: &mut Command)
  26-26      static INSTALL: std::sync::Once = std::sync::Once::new();
  45-54  pub fn hold(pid: u32)
  57-64  pub fn release(pid: u32)
  67-75  pub fn kill_group(pid: u32)
  78-84  pub fn stopped() -> bool
  88-97  pub fn code(status: ExitStatus) -> i32
  102-120  extern "C" fn forward(sig: libc::c_int)
crates/harno/tests/signals.rs
  11-11  const SESSION: &str = "0196cccc-0000-7000-8000-000000000001";
  17-17  const TREE: &str = "sh -c 'sleep 60 & echo $! > grandchild; echo $$ > child; exec sleep 61'";
  19-24  fn project(timeout: u64) -> tempfile::TempDir
  28-41  fn start(dir: &Path, ignored: Option<i32>) -> Child
  44-53  fn until<T>(mut f: impl FnMut() -> Option<T>, what: &str) -> T
  56-58  fn pid_in(dir: &Path, name: &str) -> i32
  61-65  fn gone(pid: i32)
  68-72  fn exits(dir: &Path) -> serde_json::Value
  75-90  fn a_signal_to_harno_ends_its_commands_and_all_they_started_and_the_call_with_its_line()
  93-106  fn a_signal_ignored_when_harno_started_stays_ignored()
  109-117  fn a_timeout_stops_the_command_and_all_it_started()
```

## The log: gate and sessions {1ac146}
The log of the gate, a crate of its own, `crates/harno-log` (`lib.rs`), which harno re-exports as `gate` and `log`: the log is written in one place, and harno is the only crate that takes it, since a tool names no harno crate at all and carries the little it reads (`analytics`). `gate.rs`: `.harno/log/harno/<session>.jsonl`, one line per call, written when it ends, with `n`, `ts`, `ms`, `cwd`, `verb`, `args`, and for a call that ran commands `cmd`, `exit`, `failed`, `output`, `filtered`, `lines` and `kept`; a refused call carries `refused` alone. `Gate::claim` takes the number of the call before it runs, by creating its empty `<session>/<n>.out`, so that a filtered output can name the line it will be read back from; `write_out` and `write_raw` put what the caller read and what the commands wrote beside the line, and `read_raw` is what `harno log raw <n>` reads. `log.rs`: where a log lives, a session id safe as a file name, the gitignore of the log, and what a session adds up to (`sessions`, `stats`, `total`) over the lines of the gate, with `failed`, which commands failed their call: the `failed` of its line, as the filters of that call judged each exit code, else, in a line written before that field, every exit code but 0. The words of a session are not harno: the hooks of a harness write their own log beside this one, `.harno/log/<harness>/<session>.jsonl`, which harno neither writes nor reads. Everything about Claude Code is in `tools/claude-code/`: `hook.py` (hook input and transcript into the lines of `claude-code/<session>.jsonl`, the gate lines a call claims, `CLAUDE_ENV_FILE`), `install.py` (the hooks in `.claude/settings.json`, run through the `claude-code` preset), `sandbox.py` (the sandbox block there), `only-harno.py`, `transcript.py`, `tool_config.py`, cases in its `tests/`.

Rules. The log is verbatim: nothing filtered or reinterpreted; the gate keeps the words of a call and no word of an agent or of a tool, so `rg -i "prompt|turn|agent|harness" crates/` finds nothing. The session comes from `HARNO_SESSION`; without it (a human in a terminal) the calls of a day go to the session `manual-<date>`, and only the call that starts that log warns. A hook script prints nothing and exits 1, never 2, since 2 blocks a prompt in Claude Code; what the agent wrote in a turn goes with the first call of that turn only, which the hook keeps to itself. harno writes no hook itself: what reaches `.claude/settings.json` is `install.py` through a preset.

A call of `run` is read before it ends (`cli::main`): `Gate::begin` writes `<n>.run`, how the call began (`n`, `ts`, `verb`, `args`, `cwd`), and holds it locked as long as the process lives, and `Gate::live` opens the `<n>.raw` its commands write into as they go. `write` takes `<n>.run` away with the line; the call then puts in `<n>.raw` what a filter cut, or removes it when the caller read all of it (`drop_raw`). `unfinished` reads the calls without a line: running while `<n>.run` is locked, died once the lock is free, since the lock goes with the process however it ends: a SIGKILL or a crash, since a signal harno can catch ends the call with its line (`signals.rs`). `harno log ls` lists them by number among the others, with the time they began and `running` or `died` for a result.

When changing. A new field in the line of a call: `call_line` in `cli.rs`, `MANUAL`, Guide: The log, and whatever reads it (`log::stats`, the page of `analytics`, and `stats` of mdmap, which reads `args` and lives in its own repository: a change of that field is a task there too). A new harness is a tool folder whose hooks write `<harness>/<session>.jsonl` themselves, with their `tests/` in `ci`. Tests: tests/log.rs, tests/filter.rs (what a call keeps of a filtered output), tests/config.rs (tool add and remove), `tools/claude-code/tests/`.

<!-- verify: outline crates/harno-log/src/lib.rs crates/harno-log/src/gate.rs crates/harno-log/src/log.rs crates/harno-test/tests/log.rs -->
```
crates/harno-log/src/gate.rs
  23-23  pub const GATE_DIR: &str = ".harno/log/harno";
  27-29  pub fn day_session() -> String
  31-34  pub struct Gate
  36-127  impl Gate
  37-44    pub fn open(project: &Path, session: &str) -> std::io::Result<Gate>
  46-48    pub fn path(&self) -> PathBuf
  50-52    fn text_path(&self, n: u64, ext: &str) -> PathBuf
  57-70    pub fn claim(&self) -> std::io::Result<u64>
  73-87    pub fn write(&self, n: u64, fields: Map<String, Value>) -> std::io::Result<()>
  90-92    pub fn write_out(&self, n: u64, text: &str) -> std::io::Result<()>
  95-97    pub fn write_raw(&self, n: u64, text: &str) -> std::io::Result<()>
  100-102    pub fn drop_raw(&self, n: u64)
  105-107    pub fn live(&self, n: u64) -> std::io::Result<File>
  112-126    pub fn begin(&self, n: u64, fields: Map<String, Value>) -> std::io::Result<File>
  131-147  pub fn unfinished(project: &Path, session: &str) -> Vec<(Value, bool)>
  150-155  pub fn read_raw(project: &Path, session: &str, n: u64) -> Option<String>
  158-163  pub fn read_out(project: &Path, session: &str, n: u64) -> Option<String>
crates/harno-log/src/lib.rs
  8-8  pub mod gate;
  9-9  pub mod log;
crates/harno-log/src/log.rs
  13-13  pub const LOG_DIR: &str = ".harno/log";
  15-17  pub fn now() -> String
  20-24  pub fn valid_session(s: &str) -> bool
  27-43  pub(crate) fn ensure_gitignored(project: &Path) -> std::io::Result<()>
  46-52  pub struct SessionInfo
  55-65  fn session_ids(project: &Path) -> BTreeSet<String>
  68-71  pub fn calls(project: &Path, id: &str) -> Vec<Value>
  77-82  pub fn failed(call: &Value) -> Vec<bool>
  84-86  fn ts_of(row: Option<&Value>) -> String
  90-101  pub fn sessions(project: &Path) -> Vec<SessionInfo>
  106-109  pub fn default_session(project: &Path, current: &str) -> Option<String>
  114-130  pub struct Stats
  133-147  pub fn stats(project: &Path, session: Option<&str>) -> Vec<Stats>
  151-158  pub fn total(project: &Path) -> Stats
  161-189  fn add_up(session: String, rows: &[Value]) -> Stats
crates/harno-test/tests/log.rs
  6-10  fn project() -> Project
  13-38  fn one_line_per_call_in_a_file_of_its_own()
  41-54  fn each_call_keeps_what_the_caller_read_and_counts_it()
  57-65  fn a_call_logs_its_commands_and_every_exit_code()
  68-89  fn without_a_session_the_calls_of_a_day_go_to_one_file_and_warn_once()
  92-100  fn the_log_directory_is_gitignored_when_it_is_born()
  103-122  fn log_ls_prints_the_calls_of_the_session_one_line_each()
  125-158  fn a_call_that_runs_is_in_log_ls_and_what_it_wrote_in_log_raw()
  161-178  fn a_call_that_died_before_its_line_is_listed_as_died()
  181-189  fn stats_count_the_calls_the_characters_and_the_failures()
  192-213  fn an_exit_its_filter_calls_ok_is_logged_as_no_failure_and_counted_as_none()
  216-221  fn stats_count_a_raw_read()
  224-239  fn stats_are_the_newest_session_alone_and_all_is_every_one()
  242-263  fn without_a_session_it_is_the_one_asking_and_all_puts_the_one_used_last_first()
  266-275  fn stats_json_carries_the_same_numbers_and_an_unknown_session_is_refused()
  278-295  fn analytics_ranks_the_commands_without_a_filter_of_their_own()
  298-336  fn analytics_names_the_preset_that_ran_a_command_and_all_is_every_session()
  339-347  fn analytics_says_when_there_is_nothing_to_rank()
  350-368  fn analytics_lists_the_commands_with_a_filter_and_what_it_kept()
  371-396  fn fails_count_the_commands_by_name_and_exit_and_the_refusals_by_reason()
  399-419  fn shown_per_command_cuts_a_call_at_its_headers()
```

## Analytics: the log on a page {994d28}
One read-only page of the `analytics` tool (`tools/analytics/src/serve.rs`, page `tools/analytics/assets/page.html`), bound to 127.0.0.1 and opened in the default browser unless `--no-open`: the sessions of the log, with the session list embedded. The tool reads the two logs and nothing else, and takes no harno crate: what it knows about them is `log.rs` of its own (the directory of the gate, the directory of every harness beside it, a session file, the text beside a line), `find_project` (`lib.rs`) is the nearest directory upward whose `.harno/log/` exists, and `out.rs` holds its refusal and its one text cut. `harno run analytics` serves in the calling process and ends with it, so the page dies with the terminal that started it. `--background` starts the server as a process of its own (stdio closed, its own process group), waits until it answers, writes `{pid, url}` to `.harno/log/analytics.json` and returns, so the call ends at once; a second `--background` finds the running one and opens it again, and `--stop` ends it and removes the file. The server refuses anything but GET. The logs stay on disk and the page fetches them by piece, each piece once: `/api/log/<session>` the lines of both (`?calls=<n>&events=<n>` only the later ones, for the live view), with what was written beside a prompt or an answer put back into its line, `/api/call/<session>/<n>` what a call returned and what its commands wrote, `/api/event/<session>/<n>` the input and the output of a line of the harness; `/api/sessions` serves the list again, each session with the harness whose log holds it and whether the gate logged calls of it. The log comes before the session: the picker names `harno`, the calls no harness opened, and every harness beside it, its own words with those calls under them, each with how many sessions it holds; the list under it is what the chosen log holds, and a session a harness opened belongs to that harness alone, since its log already holds those calls under the words that asked for them. A row of the list carries the time above and the name of the session under it, since under `harno` that name is only the first command: the time is the first call and, when the last falls on another minute, that one after it, its day left out when the two share one; the order is by the first call. Nothing opens by itself unless the address says so, `#src=<log>&s=<id>`; the choice of log is remembered. A session carries the green dot while a harness opened it (`started`), no end line has come and its last line is recent; calls from a plain shell log to `manual-<date>`, which no harness opens or ends, so it never carries the dot, and those sessions are what `harno` holds. The page: the session timeline on the left (filters, search, keys; `?` lists them), the selected line on the right. A tool call that went through the gate shows through the calls it claims, and a call whose output a filter shortened carries what the commands wrote under what the agent read: every line of it is marked kept or dropped, and the title says how many of its lines reached the agent. What harno ran sits folded under the call, since the call above already says what was asked for.

When changing. The page reads the shapes of both logs: a change there is a change in the page. `buildRows`, which puts the calls of the gate under the words of the session that made them, has its cases in `tools/analytics/tests/test_rows.mjs`, and `keptLines`, which says which lines of a raw output reached the agent, in `tools/analytics/tests/test_kept.mjs`; both run pieces cut out of the page, so `tools/analytics/tests/test_page.mjs` parses the whole script of it, which a browser drops entirely over one stray brace (all three in `ci`). `harno run page` with a JS expression runs the page in headless Chrome. Tests: tools/analytics/tests/routes.rs.

<!-- verify: outline tools/analytics/src/lib.rs tools/analytics/src/log.rs tools/analytics/src/main.rs tools/analytics/src/out.rs tools/analytics/src/serve.rs tools/analytics/tests/routes.rs -->

```
tools/analytics/src/lib.rs
  12-12  pub mod log;
  13-13  pub mod out;
  14-14  pub mod serve;
  21-33  pub const USAGE: &str = "usage: harno run analytics [options]
  36-45  pub fn find_project(cwd: &Path) -> Option<PathBuf>
  47-64  pub fn main(args: Vec<String>, cwd: PathBuf, out: &mut dyn Write) -> i32
  67-67  pub type Result<T> = R<T>;
tools/analytics/src/log.rs
  13-13  pub const LOG_DIR: &str = ".harno/log";
  17-17  pub const GATE: &str = "harno";
  20-22  pub fn valid_session(s: &str) -> bool
  25-27  pub fn file(project: &Path, dir: &str, session: &str) -> PathBuf
  30-32  pub fn lines(path: &Path) -> Vec<Value>
  35-40  pub fn text(project: &Path, dir: &str, session: &str, n: u64, ext: &str) -> Option<String>
  45-62  pub fn harnesses(project: &Path) -> Vec<String>
  65-67  pub fn harness_of(project: &Path, session: &str) -> Option<String>
  70-87  pub struct SessionInfo
  89-91  fn ts_of(row: Option<&Value>) -> String
  93-95  fn kind(e: &Value) -> &str
  99-151  pub fn sessions(project: &Path) -> Vec<SessionInfo>
tools/analytics/src/main.rs
  1-5  fn main()
tools/analytics/src/out.rs
  7-10  pub struct Refusal
  12-29  impl Refusal
  13-15    pub fn new(reason: impl Into<String>) -> Self
  18-21    pub fn instead(mut self, command: impl Into<String>) -> Self
  23-28    pub fn render(&self) -> String
  31-35  impl fmt::Display for Refusal
  32-34    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
  37-41  impl From<std::io::Error> for Refusal
  38-40    fn from(e: std::io::Error) -> Self
  43-43  pub type R<T> = Result<T, Refusal>;
  46-51  pub fn truncate(s: &str, max: usize) -> String
tools/analytics/src/serve.rs
  27-27  const PAGE: &str = include_str!("../assets/page.html");
  30-30  const STATE: &str = ".harno/log/analytics.json";
  33-33  type Reply = (&'static str, &'static str, Vec<u8>);
  35-111  pub fn serve(rest: &[String], project: &Path, out: &mut dyn Write) -> R<()>
  113-115  fn bind(port: u16) -> R<TcpListener>
  118-124  fn running(project: &Path) -> Option<(u64, String)>
  126-144  fn stop_server(project: &Path) -> String
  148-163  fn open_browser(url: &str)
  165-167  fn ok(kind: &'static str, body: Vec<u8>) -> Reply
  169-171  fn not_found(what: &str) -> Reply
  175-184  fn serve_routes(listener: TcpListener, route: impl Fn(&str, &str) -> Reply + Send + Sync +
  186-212  fn answer(mut stream: TcpStream, route: &dyn Fn(&str, &str) -> Reply) -> std::io::Result<()>
  214-282  fn handle(project: &Path, path: &str, query: &str) -> Reply
  285-289  fn split_n(rest: &str) -> Option<(String, u64)>
  291-297  fn sessions_json(project: &Path) -> Value
tools/analytics/tests/routes.rs
  10-10  const SESSION: &str = "0196aaaa-0000-7000-8000-000000000001";
  14-49  fn project() -> tempfile::TempDir
  51-59  fn get(port: u16, path: &str) -> (String, String)
  62-80  fn serve_in(dir: &Path, port: u16) -> std::sync::mpsc::Receiver<String>
  66-66      struct Tee(std::sync::mpsc::Sender<String>);
  67-75      impl Write for Tee
  68-71        fn write(&mut self, b: &[u8]) -> std::io::Result<usize>
  72-74        fn flush(&mut self) -> std::io::Result<()>
  82-86  fn call(dir: &Path, args: &[&str]) -> (String, i32)
  88-90  fn free_port() -> u16
  93-156  fn the_page_and_the_routes_serve_the_log_on_localhost()
  159-184  fn the_server_runs_in_the_background_until_stop()
  187-202  fn a_bad_option_and_a_directory_without_a_log_are_refused()
```

## Entry: cli, config, output {d78638}

How a call runs (`cli.rs`, `main`): `help` and `init` are handled before the project is looked up; `help` prints the usage, `help --verbose` the manual (`MANUAL`, with the built-in tools of the binary filled in by `manual`), and, in a project, every preset with its commands and every `run.allow` entry, so the agent sees what it may run; the project is the nearest directory upward with `.harno/config.yml`, else a refusal offering `harno init`. Then the session is chosen and the call takes its number from the log of the gate, `deny.calls` refuses listed calls, the verb is dispatched (`run`, `log`, `tool`), the warnings are put in front of the output or, with `--json`, into it (`in_json`), and the line of the call is written (`call_line`, `here`) with what the caller read beside it. `config.rs`: `.harno/config.yml` with `version`, `run`, `deny`, `warn`; `DEFAULT_CONFIG` is what `init` writes; unknown fields are refused; a tool keeps its settings in the `config` of its own `tool.yml`. `output.rs`: `Output` (text, code, exits, advice, the commands that ran, what they wrote before the filters, the reason a call was refused), `Refusal`, `atomic_write`, shared with the tools written in Rust.

Rules. A refusal is two lines, `refused: <reason>` and `instead: <command>` when there is a next call, and exits 1. Every verb takes `--json`; after a lone preset `--json` is an argument of the preset, as every word after it. A warning comes in front of the output; with `--json` it is one string of the `warnings` field of the object instead, without its `warning: ` prefix, so what the caller reads stays one JSON value.

When changing. A new core verb: `USAGE` (Guide: Use is verify-locked to `harno help` and follows at `refresh`) and `MANUAL` (Guide: Manual, verify-locked to `harno help --verbose`), its case in `dispatch`, a `<verb>_verb` that parses its flags; a docs verb belongs to mdmap, in its own repository. A new config field: the struct in config.rs with `#[serde(default)]` (unknown fields are refused), `DEFAULT_CONFIG`, `MANUAL`, the Guide. Tests: tests/config.rs, and each verb in its own file.

<!-- verify: outline crates/harno/src/cli.rs crates/harno/src/config.rs crates/harno/src/output.rs crates/harno/src/lib.rs crates/harno/src/main.rs crates/harno-test/tests/config.rs -->

```
crates/harno-test/tests/config.rs
  7-19  fn init_writes_every_field_with_its_default_and_nothing_else()
  22-27  fn init_does_not_overwrite()
  30-35  fn without_a_config_every_verb_points_to_init()
  38-44  fn an_unknown_version_stops_harno()
  47-54  fn an_unknown_field_is_refused_rather_than_ignored()
  57-63  fn the_project_is_found_from_a_subdirectory_and_commands_run_at_its_root()
  66-72  fn help_lists_the_verbs()
  75-84  fn an_unknown_verb_or_option_is_refused()
  87-110  fn a_tool_is_added_with_the_presets_and_the_filter_line_its_manifest_declares()
  113-123  fn tool_add_takes_several_names_and_checks_them_all_first()
  126-132  fn a_tool_that_carries_a_filter_writes_the_run_filters_line()
  135-142  fn a_preset_the_config_already_has_is_left_as_it_is()
  145-161  fn deny_calls_is_the_humans_to_change()
  164-170  fn a_long_command_output_starts_with_advice_to_filter_it()
  173-177  fn a_zero_turns_the_warning_off()
  180-186  fn help_in_a_project_lists_its_presets_and_allow()
  189-197  fn no_verb_and_the_help_flags_print_the_usage_too()
  200-208  fn help_verbose_prints_the_manual_with_the_built_in_tools()
  211-217  fn a_verb_given_more_arguments_than_it_takes_is_refused()
crates/harno/src/cli.rs
  20-26  pub struct Env
  28-36  impl Env
  29-35    pub fn from_process() -> Env
  38-66  pub const USAGE: &str = "usage: harno <verb> [args]
  70-240  pub const MANUAL: &str = r#"harno: the harness an agent works through
  243-246  fn manual() -> String
  249-410  pub fn main(args: Vec<String>, env: Env, out: &mut dyn Write) -> i32
  415-462  fn help(cwd: &Path, verbose: bool) -> String
  467-476  fn in_json(text: &str, warnings: &[String]) -> Option<String>
  480-520  fn call_line(verb: &str, rest: &[String], json: bool, ms: u64, cwd: String, output: &Output, raw: Option<&str>) -> Map<String, Value>
  523-529  fn here(project: &Path, cwd: &Path) -> String
  533-560  fn denied(config: &Config, verb: &str, rest: &[String]) -> Option<Refusal>
  562-569  fn command(verb: &str, args: &[String]) -> String
  573-579  fn dispatch(verb: &str, rest: &[String], project: &Path, config: &Config, session: &str, json: bool) -> R<Output>
  581-583  fn unknown_option(verb: &str, a: &str) -> Refusal
  585-587  fn value<'a>(rest: &'a [String], i: usize, flag: &str) -> R<&'a String>
  589-623  fn run_verb(rest: &[String], project: &Path, config: &Config, call: Option<u64>, live: Option<run::Live>, json: bool) -> R<Output>
  628-668  fn log_verb(rest: &[String], project: &Path, config: &Config, session: &str, json: bool) -> R<Output>
  673-725  fn ls_verb(rest: &[String], project: &Path, session: &str, json: bool) -> R<Output>
  731-757  fn ls_line(c: &Value) -> String
  760-766  fn size(n: u64) -> String
  770-777  fn clock(ts: &str, day: Option<&str>) -> String
  784-824  fn stats_verb(rest: &[String], project: &Path, current: &str, json: bool) -> R<Output>
  827-840  fn stats_json(s: &log::Stats) -> Value
  843-852  fn stats_block(s: &log::Stats) -> String
  856-879  fn stats_table(all: &[log::Stats], total: &log::Stats) -> String
  885-894  fn command_key(allow: &[config::Allow], words: &[run::Word]) -> String
  886-886    const SCRIPTS: [&str; 5] = [".py", ".sh", ".js", ".mjs", ".rb"];
  904-983  fn analytics_verb(rest: &[String], project: &Path, config: &Config, current: &str, json: bool) -> R<Output>
  935-935    type Row = (bool, bool, String, u64, u64, u64);
  988-1007  fn sessions_asked(rest: &[String], project: &Path, current: &str, verb: &str) -> R<(Vec<String>, usize)>
  1010-1015  fn named_presets(c: &Value) -> Vec<String>
  1024-1110  fn fails_verb(rest: &[String], project: &Path, config: &Config, current: &str, json: bool) -> R<Output>
  1027-1027    type Row = (u64, String, String);
  1114-1134  fn reason_shape(reason: &str) -> String
  1115-1115    const QUOTES: &str = "\u{27}\u{22}";
  1139-1151  fn last_line(out: &str) -> String
  1155-1168  fn tool_verb(rest: &[String], project: &Path, config: &Config, json: bool) -> R<Output>
  1171-1179  fn init(cwd: &Path) -> R<String>
crates/harno/src/config.rs
  12-12  pub const CONFIG_PATH: &str = ".harno/config.yml";
  13-13  pub const VERSION: u64 = 1;
  16-37  pub const DEFAULT_CONFIG: &str = "version: 1
  41-49  pub struct Config
  54-60  pub struct DenyConfig
  65-69  pub struct WarnConfig
  71-73  fn warn_output() -> usize
  75-79  impl Default for WarnConfig
  76-78    fn default() -> Self
  83-98  pub struct RunConfig
  105-112  pub struct SandboxConfig
  122-127  pub struct SandboxAllow
  130-132  fn everywhere() -> Vec<String>
  136-140  impl Default for SandboxAllow
  137-139    fn default() -> Self
  146-149  pub struct SandboxDeny
  153-163  pub struct Preset
  170-174  pub struct Allow
  178-181  fn strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error>
  184-193  pub fn allows<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Allow>, D::Error>
  195-202  fn command<E: serde::de::Error>(v: Value) -> Result<String, E>
  204-206  fn opt_strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error>
  209-212  fn map_or_null<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Value>, D::Error>
  214-216  fn run_timeout() -> u64
  218-229  impl Default for RunConfig
  219-228    fn default() -> Self
  231-250  impl Config
  232-249    pub fn parse(text: &str) -> R<Config>
  253-262  pub fn find_project(cwd: &Path) -> Option<PathBuf>
  264-269  pub fn load(project: &Path) -> R<Config>
  273-275  pub fn yaml_error(e: serde_saphyr::Error) -> String
  278-278  pub const ARGS: &str = "{args}";
  281-281  pub const EVERYWHERE: &str = "*";
crates/harno/src/lib.rs
  12-12  pub mod cli;
  13-13  pub mod config;
  14-14  pub mod filter;
  15-15  pub mod output;
  16-16  pub mod run;
  17-17  pub mod sandbox;
  18-18  pub mod signals;
  19-19  pub mod tool;
crates/harno/src/main.rs
  1-6  fn main()
crates/harno/src/output.rs
  10-13  pub struct Refusal
  15-41  impl Refusal
  16-18    pub fn new(reason: impl Into<String>) -> Self
  20-23    pub fn instead(mut self, command: impl Into<String>) -> Self
  25-31    pub fn render(&self) -> String
  33-40    pub fn to_json(&self) -> serde_json::Value
  43-47  impl fmt::Display for Refusal
  44-46    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
  49-53  impl From<std::io::Error> for Refusal
  50-52    fn from(e: std::io::Error) -> Self
  55-55  pub type R<T> = Result<T, Refusal>;
  59-79  pub struct Output
  81-85  impl Output
  82-84    pub fn text(text: impl Into<String>) -> Self
  89-104  pub fn atomic_write(path: &std::path::Path, text: &str) -> R<()>
  107-115  pub fn truncate(s: &str, max: usize) -> String
  118-125  pub fn dq(s: &str) -> String
  128-143  pub fn quote(s: &str) -> String
```

## Tests: harno-test and the tool tests {551eb9}

Behaviour tests drive harno in-process (`cli::main` takes args, env and stdin explicitly) against a temporary project, one file per area under `crates/harno-test/tests/`, and assert the exact output text. The analytics tests (`tools/analytics/tests/`) write a log by hand and drive `analytics::main`.

Helpers (`src/lib.rs`). `Project::new()`: `init` and an empty `docs/`. `Project::sample()`: the fixed-id corpus drawn in its doc comment. `Project::bare()`: nothing, session set. On a project: `config(yaml)`, `command(name, yaml)`, `write(rel, text)`, `read`, `exists`, `root`, `path`, `harno(&[args])`, `harno_in(cwd, …)`, `id_of(rel, title)`, `ids_in(rel)`, `calls()`. A `Call` has `code` and `out`; `ok()` asserts success and returns the text, `refused()` asserts a refusal and returns it, `instead()`, `json()`. `SAMPLE_README`, `SAMPLE_ARCH`, `SAMPLE_DESIGN`, `field(out, name)`. A call while it runs: `harno_watched(&[args])` runs it as a human in a terminal watches it and returns what reached the screen with the call; `turns()` writes `scripts/turn.sh`, a command that goes step by step (`+x` writes the line x, `@f` waits for the file f, any other step waits until the raw file of call 1 holds that line), so that a test meets a running call where it wants, with `wait_for(rel, text)` and a thread; nothing waits on a sleep. A signal needs a process of its own: `crates/harno/tests/signals.rs` starts the binary.

When changing. A behaviour change is a test change first: find the test by name in the area node. Exact asserts need the sample text: read `sample SAMPLE_README SAMPLE_ARCH SAMPLE_DESIGN` with `body` in discovery. A whole-output assert is an inline insta snapshot, `insta::assert_snapshot!(out, @"…")`: after an output change the failing `test` shows the diff, and `harno run accept` writes the new text into the test (insta ignores the trailing newline). Cargo finds a new file in `tests/` on its own; the area it tests lists it in its `verify` line. Run tests with the whole `ci`: `test -- <filter>` matches test names, not files.

<!-- verify: outline crates/harno-test/src/lib.rs -->

```
crates/harno-test/src/lib.rs
  10-10  pub const SESSION: &str = "0196aaaa-0000-7000-8000-000000000001";
  13-20  const TURN: &str = r#"for step in "$@"; do
  22-25  pub struct Project
  28-31  pub struct Call
  33-56  impl Call
  36-39    pub fn ok(&self) -> &str
  43-47    pub fn refused(&self) -> &str
  49-51    pub fn instead(&self) -> Option<String>
  53-55    pub fn json(&self) -> serde_json::Value
  58-62  impl Default for Project
  59-61    fn default() -> Self
  64-235  impl Project
  66-71    pub fn new() -> Project
  91-104    pub fn sample() -> Project
  107-112    pub fn bare() -> Project
  114-116    pub fn root(&self) -> &Path
  118-120    pub fn path(&self, rel: &str) -> PathBuf
  122-124    pub fn config(&self, yaml: &str) -> &Self
  127-129    pub fn tool(&self, name: &str, yaml: &str) -> &Self
  131-136    pub fn write(&self, rel: &str, text: &str) -> &Self
  139-141    pub fn read(&self, rel: &str) -> String
  143-145    pub fn exists(&self, rel: &str) -> bool
  147-149    pub fn harno(&self, args: &[&str]) -> Call
  151-153    pub fn harno_in(&self, cwd: &Path, args: &[&str]) -> Call
  157-163    pub fn harno_watched(&self, args: &[&str]) -> (Call, String)
  165-169    fn call(&self, env: harno::Env, args: &[&str]) -> Call
  176-178    pub fn turns(&self) -> &Self
  182-192    pub fn wait_for(&self, rel: &str, text: &str)
  196-208    pub fn id_of(&self, rel: &str, title: &str) -> String
  211-221    pub fn ids_in(&self, rel: &str) -> Vec<String>
  224-226    pub fn calls(&self) -> Vec<serde_json::Value>
  228-234    fn lines(&self, rel: &str) -> Vec<serde_json::Value>
  237-244  pub const SAMPLE_README: &str = "# Docs {a00001}
  246-264  pub const SAMPLE_ARCH: &str = "---
  266-273  pub const SAMPLE_DESIGN: &str = "# Design {a00020}
  277-287  pub fn field<T: std::str::FromStr>(out: &str, name: &str) -> T
```

## Filters: filter {ddf01d}
What a command output looks like before the agent reads it (`filter.rs`). A `run.filters` value is inline fields, a tool folder (the `filter` of its `tool.yml`; a script runs in the folder) or a filter file. The built-in filters are tools (**Tools**): a `tools/<name>/` with a `filter` in its `tool.yml` and the scripts it runs, copied to `.harno/tools/<name>/`.

Rules. Fields apply in one fixed order: colour codes removed, `replace`, `drop`, `keep`, `dedupe`, `head`/`tail`, `width`, `on_empty`; `on_fail` is the filter of a failing command; `script` hands the output to a program; `tests` run in `harno tool test`, which `ci` calls. `replace` is a list of `{pattern, with}` pairs, not a map, since a pair reads what the one before it left (owner, 2026-09-23); it rewrites lines and removes none, so a result only it changed carries no note, and the raw output stays in the log. `reads: stdout` or `stderr` gives the fields or the script that stream alone and passes the other whole, stdout before stderr: once fields have cut the lines of one stream, they cannot say where those of the other stood. Only a command such a filter may get, on success or through `on_fail` (`Filters::apart`), runs with a pipe per stream, read in the order the chunks arrive; every other command keeps one pipe and the exact order it wrote in, and the raw output is the two in the order they came. A test of such a filter gives stderr as `stderr`. The owner kept it (2026-09-23) for a script that parses structured stdout, JSON that one stderr line would break, while an error is known by the exit code and `on_fail`, not by the stream, since cargo writes its progress to stderr and a failing test its message to stdout; the name says what the filter does, where `stream` read as streaming the output. No filter of this repository reads one stream yet. `ok` is not a field of the output but of the call: it names the exit codes of that command that are no failure, so a search that found nothing answers `no match` and the chain goes on, while every other non-zero code still breaks the call; the code itself stays in the line and in the log, `ok, exit 1`. `width` reaches where the line fields cannot: one long line is all some commands write, so a line longer than it keeps its first characters and counts the rest. A filter never loses output: what the commands wrote is saved beside the line of the call, `.harno/log/harno/<session>/<n>.raw`, and the shown output ends with `harno log raw <n>`; when the result would be longer, empty or the script fails, the raw output is shown. An `on_empty` text is the exception at both ends: it stands alone, without the note, and a command that wrote nothing keeps it, since there is nothing for it to be worse than. A command filter overrides `"*"` field by field. `command` names the command a filter is written for: `harno tool ls` and `harno tool add` show it, filtering ignores it.

When changing. A new filter field: `Spec`, its `validate`, `fields`, and `merge` when the `"*"` filter may pass it down, Guide: Filtering command output. Install harno before the field is written into a config: a config harno cannot read refuses every call, its own repair included. A built-in filter is a tool whose `filter` carries `tests`; `harno tool update` brings project copies up to date. Tests: tests/filter.rs.

<!-- verify: outline crates/harno/src/filter.rs crates/harno-test/tests/filter.rs -->
```
crates/harno-test/tests/filter.rs
  9-9  const PAD: &str = "--------------------------------------------------";
  12-15  fn lines(words: &[&str]) -> String
  18-20  fn sh(words: &[&str], exit: i32) -> String
  23-25  fn seq(from: u32, to: u32) -> String
  28-46  fn project(filters: &str) -> Project
  50-53  fn raw_call(out: &str) -> String
  57-65  fn nth(out: &str, k: usize) -> Vec<String>
  67-69  fn block(out: &str) -> Vec<String>
  71-73  fn five() -> String
  76-85  fn drop_removes_lines_and_says_how_to_get_them_back()
  88-93  fn keep_keeps_only_matching_lines()
  96-104  fn dedupe_writes_a_repeated_neighbour_once()
  107-112  fn head_and_tail_cut_the_middle_and_the_note_sits_in_the_cut()
  115-123  fn head_alone_and_tail_alone()
  126-136  fn width_cuts_a_long_line_and_counts_its_characters()
  139-147  fn width_runs_after_the_fields_that_read_a_line()
  150-156  fn on_empty_speaks_when_nothing_is_left()
  159-163  fn a_command_that_wrote_nothing_speaks_through_on_empty()
  166-172  fn nothing_left_without_on_empty_shows_the_raw_output()
  175-184  fn colour_codes_and_carriage_returns_are_cleaned()
  187-192  fn fields_run_in_the_fixed_order_whatever_order_they_are_written_in()
  195-201  fn replace_rewrites_each_line_before_drop_reads_it()
  204-215  fn replace_passes_down_from_star_and_on_fail_sets_its_own()
  218-224  fn a_note_counts_the_lines_removed_and_the_lines_cut()
  227-245  fn a_filter_of_one_stream_passes_the_other_whole()
  248-252  fn a_filter_test_gives_the_other_stream_as_stderr()
  255-261  fn a_filter_that_would_make_the_output_longer_shows_the_raw_output()
  264-281  fn every_command_gets_star_and_its_own_filter_overrides_field_by_field()
  284-288  fn zero_or_false_switches_a_star_field_off()
  291-295  fn the_longest_matching_command_wins()
  298-310  fn on_fail_replaces_the_filter_when_the_command_fails()
  313-317  fn without_on_fail_the_filter_applies_to_failures_too()
  320-325  fn on_fail_takes_what_it_leaves_out_from_star()
  328-334  fn star_on_fail_is_for_commands_without_a_filter()
  336-339  fn executable(p: &Project, rel: &str, text: &str)
  342-350  fn a_script_filters_with_the_exit_code_in_harno_exit()
  353-357  fn a_script_with_arguments()
  360-370  fn a_failing_or_silent_script_shows_the_raw_output_with_a_warning()
  373-381  fn a_script_that_takes_too_long_is_stopped()
  384-389  fn a_filter_in_a_file()
  392-399  fn a_tool_folder_runs_its_script_in_the_folder()
  402-425  fn a_broken_filter_refuses_the_config()
  428-443  fn each_command_of_a_run_has_its_own_filter()
  446-452  fn a_preset_s_commands_are_each_filtered_by_their_own_filter()
  455-460  fn run_raw_is_filtered_too()
  463-481  fn the_log_keeps_what_the_agent_read_and_what_the_command_wrote()
  484-491  fn an_unfiltered_call_keeps_no_raw_output()
  494-502  fn log_raw_refuses_what_it_cannot_read()
  505-512  fn a_long_raw_output_comes_with_the_size_warning()
  515-524  fn json_output_says_which_commands_a_filter_cut()
  527-553  fn check_runs_every_filter_s_tests()
  556-564  fn a_filter_test_checks_what_the_filter_makes_of_its_input()
  567-570  fn script_filters_are_tested_too()
  575-588  fn tool_ls_lists_the_built_in_tools_and_the_project_s()
  591-603  fn tool_add_copies_the_folder_and_writes_the_lines_its_manifest_declares()
  606-624  fn an_exit_the_filter_calls_ok_is_no_failure()
  627-635  fn tool_add_refuses_what_it_cannot_do()
  638-649  fn tool_remove_takes_out_the_folder_and_leaves_the_config()
  652-665  fn tool_update_puts_back_the_binary_s_version_and_shows_the_change()
  668-682  fn tool_update_keeps_the_config_block_of_the_project()
  685-702  fn every_built_in_filter_passes_its_own_tests()
  686-686    const ALL: [&str; 8] = ["cargo-build-filter", "cargo-test-filter", "cargo-clippy-filter", "go-test-filter", "pytest-filter", "rg-filter", "bat-filter", "cargo-nextest-filter"];
  705-711  fn the_agent_uses_tool_verbs_like_any_other_call()
  715-727  fn preset_project() -> Project
  730-735  fn preset_arguments_take_the_place_of_args()
  738-744  fn json_after_a_lone_preset_is_the_preset_s()
  747-755  fn a_lone_preset_takes_every_word_after_it()
  758-764  fn a_preset_without_arguments_drops_args()
  767-774  fn arguments_need_a_preset_that_takes_them()
  777-790  fn args_inside_a_word_takes_one_plain_argument()
  793-810  fn a_default_argument_gives_way_to_the_flag_the_caller_writes()
  813-823  fn presets_in_seq_and_par_take_their_arguments_after_dashes()
  826-829  fn allowed_commands_take_arguments_without_a_preset()
  832-842  fn tool_says_what_it_takes()
crates/harno/src/filter.rs
  39-39  pub const EVERY: &str = "*";
  44-87  pub struct Spec
  93-96  pub struct Replace
  102-111  pub struct Test
  113-156  impl Spec
  114-123    fn has_fields(&self) -> bool
  126-128    fn fields(&self) -> Spec
  130-155    fn validate(&self, name: &str, top: bool) -> R<()>
  160-168  pub struct Entry
  171-173  pub struct Filters
  179-215  pub fn resolve(project: &Path, raw: &BTreeMap<String, serde_json::Value>) -> R<Filters>
  217-219  pub fn parse_spec(text: &str) -> Result<Spec, String>
  221-266  impl Filters
  222-224    fn every(&self) -> Option<&Spec>
  229-231    pub fn broke(&self, words: &[Word], exit: i32) -> bool
  235-237    pub fn apart(&self, words: &[Word]) -> bool
  241-246    pub fn entry_for(&self, words: &[Word]) -> Option<&Entry>
  251-265    pub fn choose(&self, words: &[Word], failed: bool) -> Option<Spec>
  271-296  fn merge(base: &Spec, over: &Spec) -> Spec
  298-305  fn prefix_of(entry: &[Word], words: &[Word]) -> bool
  309-323  pub struct Filtered
  327-331  pub enum Outcome
  335-350  pub fn apply(spec: &Spec, raw: &str, streams: Option<&(String, String)>, exit: i32, project: &Path, timeout: Duration, call: u64) -> Outcome
  353-375  fn transform(spec: &Spec, raw: &str, exit: i32, project: &Path, timeout: Duration) -> Outcome
  379-458  fn fields(spec: &Spec, raw: &str) -> Option<Filtered>
  461-465  fn clean(line: &str) -> String
  469-504  pub fn strip_ansi(s: &str) -> String
  508-527  pub fn render(f: &Filtered, call: u64) -> String
  530-536  fn ended(text: &str) -> String
  541-547  fn parts<'a>(spec: &Spec, raw: &'a str, streams: Option<(&'a str, &'a str)>) -> (&'a str, &'a str, &'a str)
  553-566  pub fn test_output(spec: &Spec, input: &str, stderr: &str, exit: i32, project: &Path, timeout: Duration) -> String
  569-574  pub struct Failure
  579-604  pub fn run_tests(filters: &Filters, project: &Path, timeout: Duration) -> (usize, Vec<Failure>)
  608-659  fn run_script(script: &str, dir: &Path, input: &str, exit: i32, timeout: Duration) -> Result<String, String>
  662-672  pub fn yaml_key(key: &str) -> String
```

## Dependencies {4227f6}

The crates each crate uses, one level deep; `harno run deps -- -p <crate> --depth <n>` for more. `harno run mdmap verify --fix`, in `refresh`, keeps the block current. YAML is read with `serde-saphyr`, the choice of the owner over `serde_norway` when `serde_yaml` was deprecated; it has no value type of its own, so a value whose shape is not fixed, a `run.filters` entry or a `run.allow` item, is read into a `serde_json::Value` and converted from there. Its errors carry a snippet of the file below the message, and a refusal keeps the first line (`config::yaml_error`).

<!-- verify: deps -->

```
analytics v0.1.0
└── serde_json v1.0.151

harno v0.1.0
├── glob v0.3.4
├── harno-log v0.1.0
├── libc v0.2.189
├── regex v1.13.1
├── serde v1.0.229
├── serde-saphyr v1.3.0
├── serde_json v1.0.151 (*)
└── similar v2.7.0

harno-log v0.1.0 (*)

harno-test v0.1.0
├── harno v0.1.0 (*)
├── serde_json v1.0.151 (*)
└── tempfile v3.27.0
```

## Tools: tool {0e740b}

A tool (`tool.rs`) is a folder a project adds from the ones built into harno: `tools/<name>/` with a `tool.yml`, which holds `presets` (what `add` writes into `run.presets`), `filter` (an output filter, **Filters**) and `config`, which the tool reads itself. `tool.rs` is the folder machinery as well: `build.rs` embeds the files at the top of each built-in folder; `add` copies them to `.harno/tools/<name>/`, `update` brings them back to what the binary carries and shows each change, `remove` deletes the folder; what is deeper (a crate, `tests/`) stays in the repository. A tool is no verb: it runs through a preset that names the path of its program in the folder, `analytics: { seq: [".harno/tools/analytics/bin/analytics {args}"] }`, which `add` writes from the manifest and `harno run analytics` then calls; the call is logged like any other and the tool knows nothing of the log of the gate. A tool written in Rust (`tools/analytics`) is a crate of the workspace, built into its folder (`cargo install --path tools/analytics --root .harno/tools/analytics`, in the `install` preset; `bin/` is gitignored). A program with a repository of its own is no tool: mdmap, the docs command, is installed into `~/.cargo/bin` from its repository, and the preset `mdmap: { seq: ["mdmap {args}"] }` calls it. The built-in tools: analytics, sym (Rust, TypeScript, TSX, JavaScript and Python; `roots` of its config says where it looks), edit, implement, text (a literal text, whole-word at its ends), commit-only (the named paths alone, for two sessions in one tree), claude-code, and the filters bat-filter, cargo-build-filter, cargo-clippy-filter, cargo-nextest-filter, cargo-test-filter, go-test-filter, pytest-filter, rg-filter. A filter more than one project needs is a tool, so that `harno tool update` brings every copy to the same rules and tests.

Rules. `add` copies the folder and writes into `.harno/config.yml` what the manifest declares: the `presets:` block as it is written, comments and all, and the `run.filters` line of the filter it carries. A key the config already has is left as it is, a manifest that declares neither leaves the config untouched, and a config the result does not load is put back, the whole add refused. `add` takes several names, `tool add sym edit implement`, and checks every one before it copies the first: a name that is no built-in tool, a tool the project has already and a name given twice add nothing at all. `remove` deletes the folder and leaves the config, so a folder `run.filters` still names is refused: without that the next call would be refused by a filter it cannot read. `update` keeps the `config:` block of the project and refuses a result that breaks the config. Presets call the project copies (`.harno/tools/<name>/…`), never `tools/`. A preset may be named like its program: an item that names its own preset runs the program of that name, from PATH. A tool whose whole work is a filter is named for that, `cargo-build-filter` and not `cargo-build`, so its name is not read as the command it filters. A script belongs to its tool: the Claude Code hooks to `tools/claude-code/`, the page check to `tools/analytics/tests/`; tests a tool does not ship are in its `tests/`.

Decisions, left to the agent by the owner: a project adds one kind of folder, the tool, since it has to name its program, its filter or its hooks in the config anyway. harno downloads nothing and ships no program of a tool: what is in use is built from a checkout.

When changing. A new built-in tool: a folder in `tools/` with a `tool.yml` whose `presets:` block names its program, then `harno tool add <name>`; a tool written in Rust is also a crate of the workspace and a line of the `install` preset. Tests: tests/config.rs (the tool files; a tool is no verb), tests/filter.rs (the verbs).

<!-- verify: outline crates/harno/src/tool.rs crates/harno/build.rs -->
```
crates/harno/build.rs
  10-14  fn sorted(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf>
  16-19  fn include(path: &Path) -> String
  21-37  fn tools(root: &Path, out: &Path)
  39-43  fn main()
crates/harno/src/tool.rs
  28-28  pub const DIR: &str = ".harno/tools";
  31-31  pub const MANIFEST: &str = "tool.yml";
  34-34  pub type Files = &'static [(&'static str, &'static str)];
  37-37  pub const BUILTIN: &[(&str, Files)] = include!(concat!(env!("OUT_DIR"), "/builtin_tools.rs"));
  43-52  pub struct ToolFile
  55-57  pub fn parse(text: &str) -> Result<ToolFile, String>
  60-62  pub fn valid_name(name: &str) -> bool
  65-71  fn files(name: &str) -> R<Files>
  74-76  pub fn builtin(name: &str) -> Option<&'static str>
  78-80  fn dir_of(name: &str) -> String
  82-84  fn manifest_of(name: &str) -> String
  86-88  fn added(project: &Path, name: &str) -> bool
  91-103  fn names(project: &Path) -> Vec<String>
  106-110  fn read(project: &Path, name: &str) -> R<ToolFile>
  113-115  fn filter_command(text: &str) -> Option<String>
  119-126  fn block(text: &str, key: &str) -> Option<(usize, usize)>
  129-129  type Entries = Vec<(String, Vec<String>)>;
  133-158  fn preset_entries(text: &str) -> Entries
  161-167  fn key_of(line: &str, indent: usize) -> Option<String>
  173-231  fn into_section(text: &str, section: &str, entries: &[(String, Vec<String>)]) -> (String, Vec<String>, Vec<String>)
  236-261  fn into_config(project: &Path, sections: &[(&str, Entries)]) -> R<String>
  265-276  fn keep_config(old: &str, new: &str) -> String
  279-286  fn addable(project: &Path, name: &str) -> R<Files>
  289-298  fn copy(project: &Path, name: &str) -> R<String>
  301-308  fn delete(project: &Path, name: &str) -> R<String>
  311-322  fn to_update(project: &Path, name: Option<&String>) -> R<Vec<String>>
  326-357  fn refresh(project: &Path, name: &str) -> R<(String, bool)>
  362-391  pub fn ls(project: &Path, json: bool) -> R<String>
  395-403  pub fn add_all(project: &Path, names: &[String]) -> R<String>
  409-428  fn add(project: &Path, name: &str) -> R<String>
  433-445  pub fn remove(project: &Path, config: &Config, name: &str) -> R<String>
  449-461  pub fn update(project: &Path, name: Option<&String>) -> R<String>
  465-489  pub fn test(project: &Path, config: &Config, json: bool) -> Output
  491-499  fn indent_block(label: &str, text: &str) -> String
```
