# Windows test storage

The owner requested a dedicated 320 GB Windows test area on the Linux host's
new 2 TB disk on 2026-10-12. The provisioned capacity is 320 GiB
(343,597,383,680 bytes), matching Windows' binary capacity display.

## Host and guest layout

- Host: `pb60g.bravotekcorp.cc`, existing ext4 `/data` on `/dev/sda1`.
- Image: `/data/vms/blueice-test-320g.qcow2`, created with `preallocation=falloc`;
  the host reserves the full capacity rather than relying on sparse growth.
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
