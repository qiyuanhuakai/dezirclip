"""QA-only credential syscall filter for native x86_64 and AArch64 release receipts."""
import ctypes
import errno
import hashlib
import os
from pathlib import Path
import platform
import struct

ARCH = {'x86_64': (0xc000003e, (248,249,250),39), 'aarch64': (0xc00000b7,(217,218,219),172)}

def instructions(machine):
    audit,numbers,_=ARCH[machine]
    # Reject other ABIs; x86_64 also masks x32 before the deny comparisons.
    result=[(0x20,0,0,4),(0x15,1,0,audit),(0x06,0,0,0x80000000),(0x20,0,0,0)]
    if machine=='x86_64':result.append((0x54,0,0,0xbfffffff))
    result.extend((0x15,len(numbers)-index,0,number) for index,number in enumerate(numbers))
    result.extend([(0x06,0,0,0x7fff0000),(0x06,0,0,0x00050000|errno.EPERM)])
    return result

def build_filter(machine):
    return b''.join(struct.pack('HBBI',*item) for item in instructions(machine))

def probe():
    machine=platform.machine().lower()
    if machine not in ARCH:raise RuntimeError('Only native Linux x86_64/AArch64 receipts are supported')
    status=Path('/proc/self/status').read_text()
    if 'NoNewPrivs:\t1' not in status or 'Seccomp:\t2' not in status:raise RuntimeError('Credential filter missing')
    libc=ctypes.CDLL(None,use_errno=True);libc.syscall.restype=ctypes.c_long;calls=[]
    for name,number,args in zip(('add_key','request_key','keyctl'),ARCH[machine][1],((0,0,0,0,0),(0,0,0,0),(-1,0,0,0,0))):
        ctypes.set_errno(0);value=libc.syscall(ctypes.c_long(number),*(ctypes.c_long(v) for v in args));error=ctypes.get_errno()
        if value!=-1 or error!=errno.EPERM:raise RuntimeError(name+' not denied')
        calls.append({'name':name,'number':number,'result':value,'errno':error,'arguments':'safe invalid, non-mutating'})
    if libc.syscall(ctypes.c_long(ARCH[machine][2]))!=os.getpid():raise RuntimeError('getpid must remain allowed')
    return {'status':'passed','architecture':machine,'calls':calls,'bpf_sha256':hashlib.sha256(build_filter(machine)).hexdigest(),'actual_x32_or_foreign_abi_execution':False}

def install():
    machine=platform.machine().lower()
    if machine not in ARCH or ctypes.sizeof(ctypes.c_void_p)!=8:raise RuntimeError('Unsupported native QA ABI')
    class Filter(ctypes.Structure):_fields_=[('code',ctypes.c_ushort),('jt',ctypes.c_ubyte),('jf',ctypes.c_ubyte),('k',ctypes.c_uint32)]
    class Program(ctypes.Structure):_fields_=[('length',ctypes.c_ushort),('filter',ctypes.POINTER(Filter))]
    data=build_filter(machine);array=(Filter*(len(data)//8)).from_buffer_copy(data);program=Program(len(array),array)
    libc=ctypes.CDLL(None,use_errno=True);libc.prctl.restype=ctypes.c_int;libc.prctl.argtypes=[ctypes.c_int,ctypes.c_ulong,ctypes.c_ulong,ctypes.c_ulong,ctypes.c_ulong]
    if libc.prctl(38,1,0,0,0)!=0 or libc.prctl(22,2,ctypes.addressof(program),0,0)!=0:raise RuntimeError('Credential filter installation failed')
    return probe()
