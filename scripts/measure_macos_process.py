#!/usr/bin/env python3
"""Sample an existing process; does not launch apps or change their UI."""
import argparse
import ctypes
import json
import pathlib
import platform
import re
import statistics
import subprocess
import time


class RusageInfoV0(ctypes.Structure):
    # Layout from the macOS SDK's sys/resource.h; V0 is a stable 96-byte ABI.
    _fields_ = [("ri_uuid", ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "ri_user_time", "ri_system_time", "ri_pkg_idle_wkups",
            "ri_interrupt_wkups", "ri_pageins", "ri_wired_size",
            "ri_resident_size", "ri_phys_footprint", "ri_proc_start_abstime",
            "ri_proc_exit_abstime",
        )
    ]


def physical_footprint(pid):
    info = RusageInfoV0()
    if LIBPROC.proc_pid_rusage(pid, 0, ctypes.byref(info)) != 0:
        raise OSError(ctypes.get_errno(), "proc_pid_rusage failed")
    return info.ri_phys_footprint / (1024 * 1024)


LIBPROC = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
LIBPROC.proc_pid_rusage.argtypes = (ctypes.c_int, ctypes.c_int, ctypes.c_void_p)
LIBPROC.proc_pid_rusage.restype = ctypes.c_int


def cpu_seconds(value):
    days, _, value = value.rpartition("-")
    parts = [float(part) for part in value.split(":")]
    total = 0.0
    for part in parts:
        total = total * 60 + part
    return total + (int(days) * 86400 if days else 0)


def sample(pid):
    result = subprocess.run(
        ["ps", "-p", str(pid), "-o", "rss=", "-o", "time="],
        capture_output=True, text=True, check=True,
    ).stdout.split()
    return {"at": time.monotonic(), "rss_mib": int(result[0]) / 1024,
            "cpu_seconds": cpu_seconds(result[1]),
            "physical_footprint_mib": physical_footprint(pid)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("--label", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--seconds", type=int, default=20)
    parser.add_argument("--warmup", type=int, default=5)
    parser.add_argument("--vmmap", action="store_true",
                        help="Also collect deep VM mappings after sampling; avoid during real-time audio")
    args = parser.parse_args()
    if args.pid <= 0 or args.seconds < 5 or args.warmup < 0:
        parser.error("Use a positive PID, at least five seconds, and nonnegative warmup")
    time.sleep(args.warmup)
    samples = [sample(args.pid)]
    for _ in range(args.seconds):
        time.sleep(1)
        samples.append(sample(args.pid))
    elapsed = samples[-1]["at"] - samples[0]["at"]
    cpu = samples[-1]["cpu_seconds"] - samples[0]["cpu_seconds"]
    result = {
        "label": args.label, "pid": args.pid, "platform": platform.platform(),
        "warmup_seconds": args.warmup, "sample_seconds": elapsed,
        "rss_mib_median": statistics.median(s["rss_mib"] for s in samples),
        "rss_mib_min": min(s["rss_mib"] for s in samples),
        "rss_mib_max": max(s["rss_mib"] for s in samples),
        "average_cpu_percent_one_core": cpu / elapsed * 100,
        "physical_footprint_mib": samples[-1]["physical_footprint_mib"],
        "physical_footprint_mib_median": statistics.median(s["physical_footprint_mib"] for s in samples),
        "physical_footprint_mib_min": min(s["physical_footprint_mib"] for s in samples),
        "physical_footprint_mib_max": max(s["physical_footprint_mib"] for s in samples),
        "footprint_method": "proc_pid_rusage(RUSAGE_INFO_V0)",
        "samples": samples,
    }
    # The lightweight kernel counter above does not need to inspect/suspend the task.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.vmmap:
        mapping = subprocess.run(["vmmap", "-summary", str(args.pid)], capture_output=True, text=True)
        result["vmmap_exit_code"] = mapping.returncode
        if mapping.returncode == 0:
            match = re.search(r"^Physical footprint:\s+([\d.]+)([KMG])", mapping.stdout, re.M)
            if match:
                result["vmmap_physical_footprint_mib"] = float(match[1]) * {"K": 1 / 1024, "M": 1, "G": 1024}[match[2]]
            args.output.with_suffix(".vmmap.txt").write_text(mapping.stdout)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({key: value for key, value in result.items() if key != "samples"}, indent=2))


if __name__ == "__main__":
    main()
