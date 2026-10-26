#!/usr/bin/env python3
"""FP6 USB RAM boot with padded commands; no persistent-write operations."""
import ctypes as C
import hashlib
import os
from pathlib import Path
import sys
import time

deadline = time.monotonic() + 30
while True:
    matches = [p for p in Path('/sys/bus/usb/devices').iterdir()
               if (p / 'serial').exists()
               and (p / 'serial').read_text().strip() == '147f4ef4'
               and (p / 'idVendor').read_text().strip() == '18d1']
    assert len(matches) <= 1, 'Multiple matching FP6 USB devices'
    if matches:
        USB = matches[0]
        # Enumeration precedes logind's uaccess ACL on a freshly booted USB
        # device. Wait for that existing grant before opening with libusb.
        node = '/dev/bus/usb/' + (USB / 'busnum').read_text().strip().zfill(3) + '/' + (USB / 'devnum').read_text().strip().zfill(3)
        if os.access(node, os.R_OK | os.W_OK):
            break
        assert time.monotonic() < deadline, 'FP6 fastboot USB permissions unavailable'
        time.sleep(0.1)
        continue
    assert time.monotonic() < deadline, 'FP6 fastboot USB device not found'
    time.sleep(0.5)
assert (USB / 'idVendor').read_text().strip() == '18d1'
assert (USB / 'serial').read_text().strip() == '147f4ef4'
interfaces = list(USB.glob(USB.name + ':*'))
interface = next(p for p in interfaces if
                 (p / 'bInterfaceClass').read_text().strip() == 'ff' and
                 (p / 'bInterfaceSubClass').read_text().strip() == '42' and
                 (p / 'bInterfaceProtocol').read_text().strip() == '03')
eps = [int((p / 'bEndpointAddress').read_text().strip(), 16)
       for p in interface.glob('ep_*')]
ep_in = next(e for e in eps if e & 0x80)
ep_out = next(e for e in eps if not e & 0x80)

u = C.CDLL('/nix/store/j47davc6iwnwpg8lxf72bvk1dah1qqrf-libusb-1.0.29/lib/libusb-1.0.so')
u.libusb_init.argtypes = [C.POINTER(C.c_void_p)]
u.libusb_open_device_with_vid_pid.argtypes = [C.c_void_p, C.c_uint16, C.c_uint16]
u.libusb_open_device_with_vid_pid.restype = C.c_void_p
u.libusb_get_device.argtypes = [C.c_void_p]
u.libusb_get_device.restype = C.c_void_p
u.libusb_get_bus_number.argtypes = [C.c_void_p]
u.libusb_get_device_address.argtypes = [C.c_void_p]
u.libusb_claim_interface.argtypes = [C.c_void_p, C.c_int]
u.libusb_release_interface.argtypes = [C.c_void_p, C.c_int]
u.libusb_close.argtypes = [C.c_void_p]
u.libusb_exit.argtypes = [C.c_void_p]
u.libusb_bulk_transfer.argtypes = [C.c_void_p, C.c_ubyte, C.c_void_p, C.c_int,
                                 C.POINTER(C.c_int), C.c_uint]
ctx = C.c_void_p()
assert u.libusb_init(C.byref(ctx)) == 0
handle = u.libusb_open_device_with_vid_pid(ctx, 0x18d1,
                                         int((USB / 'idProduct').read_text(), 16))
assert handle, 'Cannot open fastboot USB device (check USB permissions)'
dev = u.libusb_get_device(handle)
assert u.libusb_get_bus_number(dev) == int((USB / 'busnum').read_text())
assert u.libusb_get_device_address(dev) == int((USB / 'devnum').read_text())
iface = int((interface / 'bInterfaceNumber').read_text(), 16)
claim_deadline = time.monotonic() + 5
while True:
    claimed = u.libusb_claim_interface(handle, iface)
    if claimed == 0:
        break
    # Desktop device probes and uaccess setup can briefly race enumeration.
    # Do not detach drivers or change permissions; retry only transient states.
    assert claimed in (-3, -6) and time.monotonic() < claim_deadline, \
        f'Cannot claim FP6 fastboot interface: libusb error {claimed}'
    time.sleep(0.1)

def transfer(endpoint, data=None, size=64, timeout=10000):
    buf = C.create_string_buffer(data, len(data)) if data is not None else C.create_string_buffer(size)
    done = C.c_int()
    result = u.libusb_bulk_transfer(handle, endpoint, buf, len(buf), C.byref(done), timeout)
    if result:
        raise RuntimeError(f'USB transfer error {result}, transferred {done.value}')
    if data is not None:
        assert done.value == len(data), f'Short USB write: {done.value}/{len(data)}'
    return buf.raw[:done.value]

def response():
    while True:
        data = transfer(ep_in).rstrip(b'\0')
        print('response:', repr(data), flush=True)
        if not data.startswith(b'INFO'):
            return data

def command(cmd):
    assert cmd.startswith(('getvar:', 'download:')) or cmd in ('boot', 'reboot')
    print('command:', cmd, flush=True)
    transfer(ep_out, cmd.encode().ljust(64, b'\0'))
    return response()

try:
    assert command('getvar:product') == b'OKAYFP6'
    assert command('getvar:current-slot') == b'OKAYb'
    assert command('getvar:unlocked') == b'OKAYyes'
    if len(sys.argv) == 2 and sys.argv[1] == '--reboot':
        command('reboot')
    elif len(sys.argv) == 2:
        data = Path(sys.argv[1]).read_bytes()
        assert data.startswith(b'ANDROID!') and len(data) < 96 * 1024 * 1024
        print('image:', len(data), 'sha256:', hashlib.sha256(data).hexdigest(), flush=True)
        assert command(f'download:{len(data):08x}') == f'DATA{len(data):08x}'.encode()
        for offset in range(0, len(data), 1024 * 1024):
            transfer(ep_out, data[offset:offset + 1024 * 1024], timeout=30000)
        assert response() == b'OKAY'
        try:
            command('boot')
        except RuntimeError as error:
            if 'USB transfer error -7, transferred 0' not in str(error):
                raise
            print('Boot response timed out; verify the running OS separately.', flush=True)
    else:
        assert len(sys.argv) == 1
finally:
    u.libusb_release_interface(handle, iface)
    u.libusb_close(handle)
    u.libusb_exit(ctx)
