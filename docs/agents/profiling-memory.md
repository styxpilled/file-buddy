# Profiling memory

Three tools, from cheap to precise. Run the window from `target\debug`, and
drive it with `scripts/drive.ps1` so that each run does the same thing.

**`scripts/memory.ps1`** samples private bytes, working set, GPU memory and
handles of `file-buddy.exe` and each `fb.exe`, once a second, to a CSV:

    & ./scripts/memory.ps1 -Out <scratch>\run.csv -Seconds 300

Run it in its own process beside `drive.ps1`. Private bytes include the GPU
memory the driver commits for the process, so a rise in private bytes with a
flat working set is GPU memory, not heap.

**VMMap** (Sysinternals, `winget install Microsoft.Sysinternals.VMMap`) splits
the memory by kind: heap, private data, mapped files, images. It writes a CSV
without its window:

    Start-Process vmmap64.exe -ArgumentList '-accepteula','-p',<pid>,'<scratch>\vm.csv' -Wait

**dhat-rs** gives the Rust heap by call site. It does not see C or C++
allocations, such as ONNX Runtime, SQLite or the driver.

    cargo build --features dhat-heap

Start the window with the scratch directory as its working directory, and close
it with its close button. It writes `dhat-heap.json` there on quit, which takes
some seconds with full backtraces. The keys `gb` and `eb` hold the bytes at the
peak and at the end, per program point.

Tests cannot see any of this. The window must run.
