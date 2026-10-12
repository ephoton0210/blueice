# Windows test storage

The owner requested a dedicated 320 GB Windows test area on the Linux host's
new 2 TB disk on 2026-10-12. The provisioned capacity is 320 GiB
(343,597,383,680 bytes), matching Windows' binary capacity display.

## Host and guest layout

- Host: `pb60g.bravotekcorp.cc`, existing ext4 `/data` on `/dev/sda1`.
- Image: `/data/vms/blueice-test-320g.qcow2`, created with `preallocation=falloc`.
  Initial allocation was 343,650,058,240 bytes. The live disk uses
  `discard=unmap`, so guest discard can release those physical blocks; this is
  not a permanent host-space reservation. A later observation recorded
  31,545,245,696 allocated bytes and 1,659,417,358,336 bytes free on `/data`.
  Windows still receives the full 320 GiB virtual capacity.
- VM: `omnisolve-winui-automation`, live and persistent VirtIO disk `vda`.
- Identity: serial `BLUEICE_TEST_320G`; Windows disk 2 was verified RAW, empty,
  exactly 320 GiB and neither the boot nor system disk before initialization.
- Volume: GPT/NTFS `B:`, label `BlueIceTests320`.
- VM configuration backup:
  `/data/vms/omnisolve-winui-automation.pre-blueice-test-320g-20261012.xml`.

SATA hot attachment was refused by libvirt. The existing VirtIO ISO's Windows 11
`viostor` driver was verified with a valid Microsoft hardware signature and
installed as `oem6.inf`. VirtIO attachment succeeded without rebooting the VM.
The existing system disk and X: data disk remain attached.

## Running subsequent builds and tests

Place the checkout under `B:\BlueIce\workspaces`. Use the repository wrapper:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
    B:\BlueIce\workspaces\blueice-downloads\scripts\test-with-windows-storage.ps1 `
    -Workspace B:\BlueIce\workspaces\blueice-downloads `
    cargo +1.96.0-x86_64-pc-windows-msvc test --workspace --no-fail-fast --locked
```

The wrapper verifies the dedicated NTFS volume and refuses a checkout on
another drive. It puts Cargo build products in that checkout's `target`, Cargo,
Rustup, NuGet and .NET CLI caches under `B:\BlueIce\cache`, and TEMP/TMP under
`B:\BlueIce\tmp`. It disables incremental and dev/test debug-cache growth and
requires 50 GiB free before starting. Its environment settings apply only to
the invoked process tree and are restored afterward.

The example's execution policy is scoped to that PowerShell process; it does
not change machine or user policy. Use the same wrapper for the validation
controller and interactive desktop helper so their descendants inherit the
same paths. Pinned toolchains and package caches are prepared on B:. The wrapper
passes actual Windows checks for binary temporary-file writes, Rust 1.96
execution, C: workspace refusal, child exit-code propagation and unchanged
parent environment. This does not establish full Rust or WinUI acceptance.

A separate probe ran as the logged-in `ephoton` user in desktop session 1 with
Limited privileges. Binary TEMP I/O on B: and Rust 1.96 execution passed; its
one-time owned task exited with zero and was removed. All five B: validation
scripts pass the guest's PowerShell parser. The controller and desktop helper
both use the storage wrapper so interactive descendants receive B: paths.

## Existing evidence and cleanup

Existing X: BlueIce directories are copied to `B:\BlueIce\archive\x-drive`,
with the verified Test262 corpus in `B:\BlueIce\corpora`. Robocopy inventory
checks and source-manifest SHA256 verification are recorded under
`B:\BlueIce\results`. Archive copies exclude directory junctions; the original
X: snapshots remain available, including their original absolute-path receipts.
New checkout corpus junctions must point to the B: corpus.

The C: inventory identified six old `C:\Windows\Temp\blueice-*` trees containing
no files and 3,355 empty descendant directories. Cleanup rechecked their exact
paths, non-reparse identities and file-free subtrees before removing all 3,361
directories. Earlier conservative refusals and the incorrect initial manifest
path remain recorded as failed preparation attempts.

The SYSTEM test toolchain directory's 1,181 files, totaling 4,641,719,366 bytes,
were individually compared with the B: copy using SHA256. The original C:
`.rustup` path now has a compatibility junction to `B:\BlueIce\cache\rustup`;
Rust 1.96 execution through the original path passed before the verified old
physical copy was removed. Every previously installed toolchain is retained.
The logged-in test owner has Modify permission within `B:\BlueIce` for desktop
test descendants. Other project data and global policy were not changed.

The final probe records C: with 6.75 GiB free and B: with 292.21 GiB free. The
original 4 GiB C: acceptance guard now passes without lowering it, rebooting
the VM or changing the system pagefile. The [storage receipt](artifacts/windows-test-storage-results.json)
records exact capacities, verification counts, hashes and actual commands.

## Same-source acceptance follow-up

The committed `83436f846` checkout on B: passed all 25 workspace package
format checks. Its first full acceptance attempt stopped during Clippy when
the budget scanner encountered a Cargo temporary directory that disappeared
during enumeration. That attempt remains in `acceptance-20261012`.

The corrected scanner tolerates only missing files/directories, and NTFS
delete-pending access errors whose exact path is subsequently proven absent.
Other errors still stop validation; B: 50 GiB, C: 4 GiB and target 24 GiB
limits are unchanged. An actual guest regression completed 2,117 scans during
1,370 directory replacements and separately verified permission-error
propagation. The corrected scripts also pass the guest PowerShell parser.

The separate `acceptance-20261012-retry1` attempt has passed workspace format
and strict workspace/all-target Clippy, complete all-target build and complete
Rust execution: 5,670 passed, zero failed and 64 ignored. Its initial WinUI
build selected the GNU toolchain via `+1.96.0`;
`build.ps1` now selects `+1.96.0-x86_64-pc-windows-msvc`. A separate follow-up
uses an explicitly pinned helper against the unchanged frozen Rust/C# source
and verifies all previously completed Rust receipt hashes before reuse. The tracked follow-up receipt retains
the original stop reason, helper source hash and regression evidence.

The final same-source acceptance has now passed: WinUI native build, five
actual-Core protocol cases and seven logged-in desktop cases. Both the
desktop task and complete bootstrap exit zero; source hashes and permanent
execution policies/toolchains are unchanged. The task, helpers, frontend and
Core are gone, and frame directories were removed by ordinary window close.
The final storage audit reports C: 6.75 GiB and B: 275.28 GiB free. See the
[complete receipt](artifacts/windows-83436f846-results.json). Later API
increments require separate complete acceptance.
