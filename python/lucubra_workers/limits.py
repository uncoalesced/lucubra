# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Best-effort OS memory limits for worker subprocesses (ADR 0011 section 4)."""

from __future__ import annotations

import os
import sys

DEFAULT_MEM_LIMIT_MB = 2048
_JOB_HANDLE: object | None = None


def _apply_windows_job_limit(limit_bytes: int) -> bool:
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class _BasicLimitInfo(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.c_size_t),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class _IoCounters(ctypes.Structure):
        _fields_ = [
            ("ReadOperationCount", ctypes.c_uint64),
            ("WriteOperationCount", ctypes.c_uint64),
            ("OtherOperationCount", ctypes.c_uint64),
            ("ReadTransferCount", ctypes.c_uint64),
            ("WriteTransferCount", ctypes.c_uint64),
            ("OtherTransferCount", ctypes.c_uint64),
        ]

    class _ExtendedLimitInfo(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", _BasicLimitInfo),
            ("IoInfo", _IoCounters),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        ]

    job_extended_limit_info = 9
    limit_process_memory = 0x00000100

    kernel32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
    kernel32.CreateJobObjectW.restype = wintypes.HANDLE
    kernel32.SetInformationJobObject.argtypes = [
        wintypes.HANDLE,
        ctypes.c_int,
        wintypes.LPVOID,
        wintypes.DWORD,
    ]
    kernel32.SetInformationJobObject.restype = wintypes.BOOL
    kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
    kernel32.GetCurrentProcess.restype = wintypes.HANDLE
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

    job = kernel32.CreateJobObjectW(None, None)
    if not job:
        return False

    info = _ExtendedLimitInfo()
    info.BasicLimitInformation.LimitFlags = limit_process_memory
    info.ProcessMemoryLimit = limit_bytes

    if not kernel32.SetInformationJobObject(
        job,
        job_extended_limit_info,
        ctypes.byref(info),
        ctypes.sizeof(info),
    ):
        kernel32.CloseHandle(job)
        return False

    if not kernel32.AssignProcessToJobObject(job, kernel32.GetCurrentProcess()):
        kernel32.CloseHandle(job)
        return False

    global _JOB_HANDLE
    _JOB_HANDLE = job
    return True


def _apply_posix_rlimit(limit_bytes: int) -> bool:
    try:
        import resource
    except ImportError:
        return False
    try:
        soft, hard = resource.getrlimit(resource.RLIMIT_AS)
        target = limit_bytes if hard == resource.RLIM_INFINITY else min(limit_bytes, hard)
        if soft == resource.RLIM_INFINITY or soft > target:
            resource.setrlimit(resource.RLIMIT_AS, (target, hard))
        return True
    except (ValueError, OSError):
        return False


def apply_memory_limit(limit_mb: int | None = None) -> bool:
    """Apply the configured memory limit to the current worker process."""
    if limit_mb is None:
        raw = os.environ.get("LUCUBRA_WORKER_MEM_MB", str(DEFAULT_MEM_LIMIT_MB))
        try:
            limit_mb = int(raw)
        except ValueError:
            return False
    if limit_mb <= 0:
        return False
    limit_bytes = limit_mb * 1024 * 1024
    if sys.platform == "win32":
        return _apply_windows_job_limit(limit_bytes)
    return _apply_posix_rlimit(limit_bytes)
