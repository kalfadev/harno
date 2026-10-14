# Roadmap {d80c9a}

What comes next for harno, in plain words. An item leaves this page when it is built and the docs describe it, or when the owner drops it.

## Prove that it saves something {0ff2d2}

Run the same tasks with harno and without it, and compare the time, the calls and the characters the model read. The log already has the numbers. What is missing is the method: which tasks, how many runs, and what counts as a win. This is the item that says whether harno is worth using.

## Less noise in what a call returns {4a9bc3}

A call that runs several commands prints a header line for each one. A passing `implement` is mostly headers, one per check. Find what the agent reads back that tells it nothing new, such as a header that repeats the command it just wrote, and cut it.

## A clearer log page {3d35ca}

The analytics page shows the `harno` log and the log of each harness separately, and one session can show up in both. Decide which log the page opens with, how a session without a harness is named, and what an empty log says.

## Filters that follow you {366bda}

Today a filter lives in the project. rtk and snip, two other output filters, also look in the home directory, so a filter written once works in every project. Decide whether harno should do the same.

## More agents {2d18da}

harno comes with hooks for Claude Code only. Another agent needs a tool folder whose hook scripts write its session log beside the log of harno.

## Releases {0fb829}

Releases build harno for macOS, Linux and Windows, and GitHub Actions runs the tests on Linux and macOS and starts harno on Windows. Next: the whole test suite on Windows, whose tests start `sh` today, and the Claude Code hooks tried there. Later: publish to crates.io.
