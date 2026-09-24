"""Windows user-scoped data protection shared by local maintenance tools."""
import ctypes
from ctypes import wintypes


class Blob(ctypes.Structure):
    _fields_ = [('length', wintypes.DWORD), ('data', ctypes.POINTER(ctypes.c_ubyte))]


def crypt(data, protect):
    buffer = ctypes.create_string_buffer(data)
    source = Blob(len(data), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_ubyte)))
    target = Blob()
    api = ctypes.windll.crypt32
    if protect:
        ok = api.CryptProtectData(ctypes.byref(source), None, None, None, None, 1, ctypes.byref(target))
    else:
        ok = api.CryptUnprotectData(ctypes.byref(source), None, None, None, None, 1, ctypes.byref(target))
    if not ok:
        raise RuntimeError('Windows credential protection failed')
    try:
        return ctypes.string_at(target.data, target.length)
    finally:
        ctypes.windll.kernel32.LocalFree(target.data)
