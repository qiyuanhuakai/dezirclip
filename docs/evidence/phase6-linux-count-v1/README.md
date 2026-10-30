# Linux count-v1 offline reproduction

From the repository root, run with Python 3 and its standard library:

```sh
python docs/evidence/phase6-linux-count-v1/verify-linux-count-v1.py docs/evidence/phase6-linux-count-v1/phase6-linux-count-v1-inputs.zip
```

The verifier reads ZIP members directly. It does not extract files, execute or import bundled source, invoke products, use the network, or access devices.

The archive contains 164 unchanged original files plus `manifest.json`: all 136 raw records from the four `5d6af007` count-v1 arms, 27 non-ELF analysis/proof dependencies, and the separately referenced frozen pacer source. The manifest maps original provenance paths to unique relative ZIP paths and records roles, byte lengths and SHA-256 values.

The analysis rechecks included bytes, 120 formal activation proxy latencies, 7,680 process-family memory/CPU/thread captures, 7,680 clock rows, and per-arm/pooled nearest-rank statistics against the frozen aggregate. Integer counters, nanosecond timestamps, byte lengths and hashes require exact equality. Derived floating-point comparisons use relative tolerance `1e-12` and absolute tolerance `1e-9`; interpreter-dependent rounding of a mean does not modify the original data.

CPU normalization uses disjoint first/last counter-read midpoint intervals. Memory distributions describe actual count-paced snapshots, rather than a time-weighted 240-second mean. Context-switch counts are not wakeups. The [performance report](../../specs/phase6-performance.md) records the display/compositing restrictions and remaining acceptance gaps.

Seven ELF inputs are absent: Desktop, MyGo helper, private Secret Service helper, WebKit driver, Python, screenshot tool and paste tool. Their historical identities remain in the manifest and original records; this verifier does not rehash absent bytes or recertify builds. Four small input proof/note files are included. Original `assess.py` and `summarize-cohort.py` remain unchanged review material, with their original host paths and input checks. The separate portable entry point implements the stated statistics, clocks and included-inventory checks.

Application text is the fixed ten-row synthetic workload. The unchanged records also contain local usernames/paths, process and namespace identities, and bounded host-load metadata. The product environment is a fixed 12-entry fixture whitelist. Product profiles/databases, Xauthority, private Secret Service state, IPC credentials and ELF bytes are excluded; Secret Service records are process/status receipts.

The archive is 4,846,239 bytes, SHA-256 `3b2eaa1867b7bfe1fbec4fa7cf18626b279a71f258809ef66215e9facfa4f994`. The verifier's canonical LF source SHA-256 is `ce0b8a8bb24336687480596de75e3b6d30ef5959ab273b0e4e9f164c1230bbd6`.

Offline reproduction does not rerun native acceptance, isolation or image inspection. It does not establish GPU usage, wakeups, first interactive frame, IME/DPI/multimonitor behavior, Windows performance, Phase 6 acceptance or Phase 7 authorization. Earlier failed cohorts remain separate; Tauri remains the default.
