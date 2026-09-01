#!/usr/bin/env python3
import argparse
import hashlib
import http.server
import json
import math
import os
import platform
import statistics
import subprocess
import tempfile
import threading
from datetime import datetime, timezone
from pathlib import Path


HERE = Path(__file__).resolve().parent
BUDGET_MIPS = 480.0
PIXELS = 2048
INSTRUCTIONS_PER_ITERATION = 8 * PIXELS + 3
SAMPLE_SECONDS = 1.5
SAMPLE_ORDER = [
    "off", "on", "on", "off", "off", "on", "on",
    "off", "off", "on", "on", "off", "off", "on",
]
METHODOLOGY = {
    "clock": "performance.now",
    "summaryStatistic": "median of seven samples per mode",
    "ordering": "paired ABBA-style interleave",
    "accountingOn": "inlined D-cache tag checks, miss classification, and 64-bit synthetic cycle ledger",
    "accountingOff": "identical architectural kernel with accounting compiled out",
    "timingScope": "cache misses counted but unpriced; first-line fill remains blocked",
}
CHROME_DEFAULT = Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")


def command(*args: str) -> str:
    return subprocess.run(args, check=True, text=True, stdout=subprocess.PIPE).stdout.strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build(output: Path) -> dict:
    zig = os.environ.get("ESP32SIM_ZIG", "zig")
    common = [
        zig,
        "cc",
        "-std=c17",
        "-O2",
        "-Wall",
        "-Wextra",
        "-Werror",
        str(HERE / "jit_probe.c"),
        "-target",
        "wasm32-freestanding",
        "-ffreestanding",
        "-fno-builtin",
        "-Wl,--no-entry",
        "-Wl,--export=jit_setup",
        "-Wl,--export=jit_run",
        "-Wl,--export=jit_cycles_lo",
        "-Wl,--export=jit_cycles_hi",
        "-Wl,--export=jit_misses_lo",
        "-Wl,--export=jit_misses_hi",
        "-Wl,--export=jit_dest",
        "-Wl,--export-memory",
        "-Wl,--strip-all",
    ]
    artifacts = {}
    for name, enabled in (("accounting-off.wasm", "0"), ("accounting-on.wasm", "1")):
        destination = output / name
        subprocess.run([*common, f"-DCYCLE_ACCOUNTING={enabled}", "-o", str(destination)], check=True)
        artifacts[name] = destination
    return {
        "compiler": f"zig {command(zig, 'version')}",
        "sourceSha256": sha256(HERE / "jit_probe.c"),
        "benchmarkJsSha256": sha256(HERE / "benchmark.js"),
        "benchmarkSpecSha256": sha256(HERE / "benchmark-spec.json"),
        "indexHtmlSha256": sha256(HERE / "index.html"),
        "runnerSha256": sha256(HERE / "run-chrome.py"),
        "smokeSha256": sha256(HERE / "smoke.mjs"),
        "accountingOffWasmSha256": sha256(artifacts["accounting-off.wasm"]),
        "accountingOnWasmSha256": sha256(artifacts["accounting-on.wasm"]),
    }


def median(values: list[float]) -> float:
    return statistics.median(values)


def validate_samples(raw: dict) -> None:
    expected_orders = set(range(14))
    expected_mode_orders = {
        "accountingOff": {0, 3, 4, 7, 8, 11, 12},
        "accountingOn": {1, 2, 5, 6, 9, 10, 13},
    }
    seen_orders = set()
    checksums = set()
    output_hashes = set()
    calibration = raw.get("calibration", {})
    for key, accounting_on in (("accountingOff", False), ("accountingOn", True)):
        samples = raw.get(key)
        mode = "on" if accounting_on else "off"
        mode_calibration = calibration.get(mode, {})
        if mode_calibration.get("probeIterations") != 4000:
            raise ValueError(f"{key} calibration probe count is inconsistent")
        probe_elapsed = mode_calibration.get("probeElapsedMilliseconds")
        selected_iterations = mode_calibration.get("selectedIterations")
        if not isinstance(probe_elapsed, (int, float)) or not math.isfinite(probe_elapsed) or probe_elapsed <= 0:
            raise ValueError(f"{key} calibration duration is invalid")
        if not isinstance(selected_iterations, int) or selected_iterations <= 0:
            raise ValueError(f"{key} calibrated iteration count is invalid")
        expected_iterations = math.floor(4000 * SAMPLE_SECONDS * 1000 / probe_elapsed + 0.5)
        expected_iterations = max(1, min(0x7FFFFFFF, expected_iterations))
        if selected_iterations != expected_iterations:
            raise ValueError(f"{key} calibrated iteration count does not recompute")
        if not isinstance(samples, list) or len(samples) != 7:
            raise ValueError(f"{key} must contain seven samples")
        mode_orders = {sample["orderIndex"] for sample in samples}
        if mode_orders != expected_mode_orders[key]:
            raise ValueError(f"{key} sample order is inconsistent")
        for sample in samples:
            iterations = sample["iterations"]
            instructions = sample["emulatedInstructions"]
            elapsed_ms = sample["elapsedMilliseconds"]
            measured_mips = sample["mips"]
            if not isinstance(iterations, int) or iterations <= 0:
                raise ValueError(f"{key} has an invalid iteration count")
            if iterations != selected_iterations:
                raise ValueError(f"{key} did not use its calibrated iteration count")
            if instructions != iterations * INSTRUCTIONS_PER_ITERATION:
                raise ValueError(f"{key} instruction count is inconsistent")
            if not math.isfinite(elapsed_ms) or elapsed_ms <= 0:
                raise ValueError(f"{key} elapsed time is invalid")
            if not SAMPLE_SECONDS * 750 <= elapsed_ms <= SAMPLE_SECONDS * 2000:
                raise ValueError(f"{key} elapsed time is outside the target-run tolerance")
            recomputed_mips = instructions / elapsed_ms / 1000
            if not math.isclose(measured_mips, recomputed_mips, rel_tol=1e-12):
                raise ValueError(f"{key} MIPS does not recompute")
            if accounting_on:
                expected_cycles = iterations * INSTRUCTIONS_PER_ITERATION
                if sample["cycleLedger"] != expected_cycles:
                    raise ValueError(f"{key} cycle ledger is inconsistent")
                if sample["cacheMisses"] != 128:
                    raise ValueError(f"{key} cache miss count is inconsistent")
            elif sample["cycleLedger"] != 0 or sample["cacheMisses"] != 0:
                raise ValueError(f"{key} changed the disabled cycle ledger")
            seen_orders.add(sample["orderIndex"])
            checksums.add(sample["checksum"])
            output_hashes.add(sample["outputFnv1a32"])
    if seen_orders != expected_orders:
        raise ValueError("sample order indices are incomplete")
    if len(checksums) != 1 or next(iter(checksums)) == 0:
        raise ValueError("architectural checksums differ or are zero")
    if output_hashes != {expected_output_hash()}:
        raise ValueError("full architectural output hashes differ or are invalid")


def expected_output_hash() -> int:
    value = 0x811C9DC5
    for index in range(0, PIXELS * 2, 2):
        low = (index * 31 + 7) & 0xFF
        high = ((index + 1) * 31 + 7) & 0xFF
        for byte in (high, low):
            value ^= byte
            value = (value * 0x01000193) & 0xFFFFFFFF
    return value


def derive(raw: dict) -> dict:
    off = median([sample["mips"] for sample in raw["accountingOff"]])
    on = median([sample["mips"] for sample in raw["accountingOn"]])
    return {
        "accountingOffMedianMips": off,
        "accountingOnMedianMips": on,
        "accountingCostPercent": 100 * (1 - on / off),
        "realTimeBudgetMips": BUDGET_MIPS,
        "clearsBudget": on >= BUDGET_MIPS,
        "marginMips": on - BUDGET_MIPS,
        "marginPercent": 100 * (on / BUDGET_MIPS - 1),
    }


def validate_report(report: dict, expected_toolchain: dict) -> None:
    if report.get("schemaVersion") != 1 or report.get("status") != "measured":
        raise ValueError("unsupported or incomplete result")
    host = report.get("host", {})
    if host.get("cpu") != "Apple M1 Pro" or host.get("architecture") != "arm64":
        raise ValueError("result is not from the target Apple M1 Pro")
    if "Chrome" not in report.get("browser", {}).get("version", ""):
        raise ValueError("result is not from Google Chrome")
    parameters = report.get("parameters", {})
    if parameters.get("pixelsPerIteration") != PIXELS:
        raise ValueError("pixel count is inconsistent")
    if parameters.get("emulatedInstructionsPerIteration") != INSTRUCTIONS_PER_ITERATION:
        raise ValueError("instruction count is inconsistent")
    if parameters.get("targetSecondsPerSample") != SAMPLE_SECONDS:
        raise ValueError("sample duration is inconsistent")
    if parameters.get("sampleOrder") != SAMPLE_ORDER:
        raise ValueError("declared sample order is inconsistent")
    if report.get("toolchain") != expected_toolchain:
        raise ValueError("result does not match the committed harness or rebuilt wasm artifacts")
    if report.get("methodology") != METHODOLOGY:
        raise ValueError("recorded methodology is inconsistent")
    validate_samples(report["raw"])
    expected = derive(report["raw"])
    for key, value in expected.items():
        actual = report["derived"].get(key)
        if isinstance(value, bool):
            if actual is not value:
                raise ValueError(f"derived {key} is inconsistent")
        elif not math.isclose(actual, value, rel_tol=1e-12, abs_tol=1e-12):
            raise ValueError(f"derived {key} is inconsistent")


class ResultServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, handler, artifacts):
        super().__init__(address, handler)
        self.artifacts = artifacts
        self.result = None
        self.error = None
        self.done = threading.Event()


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, _format, *_args):
        pass

    def do_GET(self):
        files = {
            "/": (HERE / "index.html", "text/html; charset=utf-8"),
            "/benchmark.js": (HERE / "benchmark.js", "text/javascript; charset=utf-8"),
            "/accounting-off.wasm": (self.server.artifacts / "accounting-off.wasm", "application/wasm"),
            "/accounting-on.wasm": (self.server.artifacts / "accounting-on.wasm", "application/wasm"),
        }
        item = files.get(self.path)
        if item is None:
            self.send_error(404)
            return
        path, content_type = item
        body = path.read_bytes()
        self.send_response(200)
        self.send_header("content-type", content_type)
        self.send_header("cache-control", "no-store")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        try:
            length = int(self.headers.get("content-length", "0"))
            if length <= 0 or length > 1024 * 1024:
                raise ValueError("invalid result length")
            payload = json.loads(self.rfile.read(length))
            if self.path == "/result":
                self.server.result = payload
            elif self.path == "/error":
                self.server.error = payload.get("error", "unknown browser error")
            else:
                self.send_error(404)
                return
            self.send_response(204)
            self.end_headers()
        except Exception as error:
            self.server.error = str(error)
            self.send_error(400)
        finally:
            self.server.done.set()


def target_host() -> dict:
    architecture = platform.machine()
    cpu = command("sysctl", "-n", "machdep.cpu.brand_string")
    if architecture != "arm64" or cpu != "Apple M1 Pro":
        raise RuntimeError(f"target is Apple M1 Pro arm64, found {cpu} {architecture}")
    return {
        "cpu": cpu,
        "architecture": architecture,
        "os": f"macOS {command('sw_vers', '-productVersion')} ({command('sw_vers', '-buildVersion')})",
    }


def run_measurement() -> None:
    host = target_host()
    chrome = Path(os.environ.get("ESP32SIM_CHROME", str(CHROME_DEFAULT))).resolve()
    if not chrome.is_file():
        raise RuntimeError(f"Google Chrome not found at {chrome}; set ESP32SIM_CHROME")
    chrome_version = command(str(chrome), "--version")
    if "Google Chrome" not in chrome_version:
        raise RuntimeError(f"expected Google Chrome, found {chrome_version}")

    with tempfile.TemporaryDirectory(prefix="esp32sim-jit-spike-") as temporary:
        temporary_path = Path(temporary)
        artifacts = temporary_path / "artifacts"
        artifacts.mkdir()
        build_receipt = build(artifacts)
        server = ResultServer(("127.0.0.1", 0), Handler, artifacts)
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        url = f"http://127.0.0.1:{server.server_port}/"
        process = subprocess.Popen(
            [
                str(chrome),
                "--headless=new",
                f"--user-data-dir={temporary_path / 'chrome-profile'}",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-renderer-backgrounding",
                "--disable-background-timer-throttling",
                "--disable-backgrounding-occluded-windows",
                url,
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            if not server.done.wait(timeout=180):
                stderr = process.stderr.read() if process.poll() is not None else ""
                raise RuntimeError(f"Chrome measurement timed out: {stderr.strip()}")
            if server.error:
                raise RuntimeError(f"Chrome measurement failed: {server.error}")
            browser_result = server.result
        finally:
            server.shutdown()
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    raw = browser_result["raw"]
    validate_samples(raw)
    report = {
        "schemaVersion": 1,
        "status": "measured",
        "measuredAtUtc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "host": host,
        "browser": {
            "version": chrome_version,
            "userAgent": browser_result["browser"]["userAgent"],
            "language": browser_result["browser"]["language"],
        },
        "toolchain": build_receipt,
        "parameters": browser_result["parameters"],
        "methodology": METHODOLOGY,
        "raw": raw,
        "derived": derive(raw),
    }
    validate_report(report, build_receipt)
    destination = HERE / "result.json"
    temporary_destination = destination.with_suffix(".json.tmp")
    temporary_destination.write_text(json.dumps(report, indent=2) + "\n")
    temporary_destination.replace(destination)
    derived = report["derived"]
    verdict = "clears" if derived["clearsBudget"] else "misses"
    print(f"wrote {destination}")
    print(
        f"accounted JIT spike {derived['accountingOnMedianMips']:.2f} MIPS, {verdict} "
        f"480 MIPS by {derived['marginMips']:.2f} MIPS ({derived['marginPercent']:.2f}%)"
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="esp32sim-jit-smoke-") as temporary:
        output = Path(temporary)
        receipt = build(output)
        subprocess.run(
            ["node", str(HERE / "smoke.mjs"), str(output / "accounting-off.wasm"), str(output / "accounting-on.wasm")],
            check=True,
        )
        print(json.dumps(receipt, indent=2))


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the wasm JIT accounting spike in target Chrome")
    parser.add_argument("--self-test", action="store_true", help="compile and verify under Node without measuring")
    parser.add_argument("--validate", type=Path, help="validate an existing target result")
    arguments = parser.parse_args()
    if arguments.self_test:
        self_test()
    elif arguments.validate:
        with tempfile.TemporaryDirectory(prefix="esp32sim-jit-validate-") as temporary:
            expected_toolchain = build(Path(temporary))
        validate_report(json.loads(arguments.validate.read_text()), expected_toolchain)
        print(f"valid: {arguments.validate}")
    else:
        run_measurement()


if __name__ == "__main__":
    main()
