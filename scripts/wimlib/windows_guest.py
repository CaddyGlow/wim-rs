"""Bounded QGA file transfer and execution for an explicitly selected owned guest."""
import base64
import fcntl
import json
import pathlib
import secrets
import socket
import time


class _SynchronizationFailure(RuntimeError):
    pass


class WindowsGuest:
    def __init__(self, socket_path):
        self.socket_path = str(socket_path)

    def call(self, command, arguments=None):
        # A QEMU serial socket accepts a single active client; independent
        # probes must serialize their RPC connections to this selected guest.
        with open(self.socket_path + '.client.lock', 'a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            return self._call_locked(command, arguments)

    def _call_locked(self, command, arguments):
        for attempt in range(3):
            try:
                return self._call_once(command, arguments)
            except _SynchronizationFailure:
                if attempt == 2:
                    raise
                time.sleep(.05)

    def _call_once(self, command, arguments):
        with socket.socket(socket.AF_UNIX) as channel:
            channel.settimeout(10)
            try:
                channel.connect(self.socket_path)
            except (TimeoutError, BlockingIOError) as error:
                raise _SynchronizationFailure('guest connection failed before RPC') from error
            # QGA requires synchronization on every newly connected stream.
            # The sentinel also resets stale partial input after a timeout:
            # https://www.qemu.org/docs/master/interop/qemu-ga-ref.html#command-guest-sync-delimited
            identity = secrets.randbits(63)
            sync = {'execute': 'guest-sync-delimited', 'arguments': {'id': identity}}
            channel.sendall(b'\xff' + (json.dumps(sync) + '\n').encode())
            with channel.makefile('rb') as reader:
                try:
                    while True:
                        marker = reader.read(1)
                        if not marker:
                            raise _SynchronizationFailure('guest agent closed before synchronization')
                        if marker == b'\xff':
                            synchronized = json.loads(reader.readline())
                            if synchronized.get('return') == identity:
                                break
                except TimeoutError as error:
                    raise _SynchronizationFailure('guest synchronization failed before RPC') from error
                # Only connection/synchronization failures are retried. Once an
                # RPC is sent, a timeout must propagate: mutation may have run.
                channel.settimeout(60)
                request_id = secrets.randbits(63)
                channel.sendall((json.dumps({'execute': command, 'arguments': arguments or {}, 'id': request_id}) + '\n').encode())
                while True:
                    result = json.loads(reader.readline())
                    if result.get('id') == request_id:
                        break
        if 'error' in result:
            raise RuntimeError(result['error'])
        return result['return']

    def execute(self, path, arguments, timeout=300):
        pid = self.call('guest-exec', {'path': path, 'arg': arguments, 'capture-output': True})['pid']
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            result = self.call('guest-exec-status', {'pid': pid})
            if result.get('exited'):
                return {'exit': result.get('exitcode'), 'signal': result.get('signal'),
                        'stdout_base64': result.get('out-data', ''),
                        'stdout': base64.b64decode(result.get('out-data', '')).decode('utf-8', 'replace'),
                        'stderr': base64.b64decode(result.get('err-data', '')).decode('utf-8', 'replace'),
                        'stdout_truncated': result.get('out-truncated', False)}
            time.sleep(.5)
        raise TimeoutError(f'guest process {pid} did not exit')

    def powershell(self, command):
        return self.execute(r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
                            ['-NoProfile', '-NonInteractive', '-Command',
                             '[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); ' + command])

    def put(self, path, data):
        handle = self.call('guest-file-open', {'path': path, 'mode': 'wb'})
        try:
            for offset in range(0, len(data), 65536):
                chunk = data[offset:offset + 65536]
                written = self.call('guest-file-write', {'handle': handle, 'buf-b64': base64.b64encode(chunk).decode()})
                if written['count'] != len(chunk):
                    raise RuntimeError('short guest write')
        finally:
            self.call('guest-file-close', {'handle': handle})

    def get(self, path):
        result = bytearray()
        handle = self.call('guest-file-open', {'path': path, 'mode': 'rb'})
        try:
            while True:
                chunk = self.call('guest-file-read', {'handle': handle, 'count': 65536})
                result.extend(base64.b64decode(chunk.get('buf-b64', '')))
                if chunk['eof']:
                    return bytes(result)
        finally:
            self.call('guest-file-close', {'handle': handle})
