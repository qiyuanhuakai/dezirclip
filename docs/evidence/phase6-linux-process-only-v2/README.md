# Linux process-resource-only-v2 offline reproduction

From the repository root, use Python 3.13 and its standard library:

```sh
python -s -B docs/evidence/phase6-linux-process-only-v2/verify-linux-process-only-v2.py docs/evidence/phase6-linux-process-only-v2/phase6-linux-process-only-v2-inputs.zip
```

The [portable verifier](verify-linux-process-only-v2.py) reads the [ZIP](phase6-linux-process-only-v2-inputs.zip) directly. It never extracts or imports archived source, starts a product, invokes a subprocess, uses the network, or accesses devices. The verifier is an unchanged copy of the separately reviewed local source; its exact bytes are checked against the archived copy. Its scoped Git attribute keeps LF on Windows checkouts.

This is the complete new `1fe685c424602b2bc1892f1deb0794c2e0b90ad5` cohort, in Tauri/MyGo/MyGo/Tauri order. Each arm has 5 warmups, 30 formal native activation proxy observations, and 960 actual resource captures per visible/hidden state. All 120 formal observations, 7,680 resource records and 7,680 original clock records are recomputed. The former failed thread-sampling A1 and the older `5d6af007` cohort are separate and contribute no samples.

The archive contains 243 unchanged original files plus `manifest.json`: 136 closed raw records, 58 cohort control/assessment/PNG/review files, 38 frozen source files, and 11 proof/audit files. The manifest maps 349 literal original provenance paths to those unique members, with byte lengths, roles and SHA-256. Neither original JSON paths nor original hashes were rewritten. Sources include the frozen sampler, integration, independent reader and independent summary audit; they are review material, never executed by the portable verifier. Saved prerequisite/native/credential/cleanup/PNG-view receipts are checked as records, without reenacting their original acceptance.

The [original machine-readable summary](../phase6-linux-process-only-v2.json) is 92,439 bytes, SHA-256 `3c9e9b1d8162859efc11aa5a30cede300b5bf77418559c52f5cb389367807b86`. The separately written [independent summary audit](independent-summary-audit.json) is 74,139 bytes, SHA-256 `39044e15012b6e4a31e9a97af33fa8260a0edbf456e05225144f2e0ed0b4e04c`; both are also included unchanged in the ZIP. The verifier independently recomputes from the raw members before comparing these summaries.

All included bytes and closed inventories are checked. Integer counters, nanoseconds and hashes require exact equality; derived floating-point comparisons use relative tolerance `1e-12` and absolute tolerance `1e-9`. Memory distributions describe count-paced snapshots, not a time-weighted window average. Pooled CPU uses summed deltas divided by summed first/last counter-read intervals, retaining the original read-window normalization range. It is not a confidence interval or full-window/tail CPU. Thread counts, context switches and wakeups are not collected and remain null. Removing thread probes changes observer cost; cross-method differences do not establish product improvement.

Seven ELF inputs are absent: Desktop, MyGo helper, private Secret Service helper, WebKit driver, Python, screenshot tool and paste tool. Their historical full-file SHA/size and separately observed 64-byte ELF header identities are retained, but absent complete bytes are not rehashed by this verifier. Four non-ELF input proof/note records are included; one is already a cohort control file. Rebuilding native acceptance requires the original binaries and environment.

The workload is the fixed ten-row synthetic text fixture. Unchanged records include local usernames/paths, original process/namespace identities and bounded host metadata. Profiles, clipboard databases, private Secret Service storage, Xauthority, sockets and executable/build payloads are excluded. The explicit `private-ss/proof.json` members contain process/status receipts, not credential storage.

ZIP: 2,189,789 bytes, SHA-256 `130f8a7c4d728407323d7131e97bb47dc9f89095814daefa3c7666e30e3b2178`. Verifier: 67,646 bytes, SHA-256 `10cc645d3d21bfd5f32c287440e38be37a78c522716683c081b61caf8ad62486`. Source review and 37 pure tests preceded one assembly invocation and its original offline verification; the native benchmarks were not rerun for packaging.

The [performance report](../../specs/phase6-performance.md) gives the restricted Xvfb/compositing configuration, per-arm results and decision. Offline consistency does not establish host quiet, hardware GPU usage, first interactive frame, search readiness, IME/DPI/multimonitor behavior, statistical significance, historical crash repair or default migration. Phase6 remains unaccepted, Phase7 is not started, and Tauri stays the default.
