"""Owned PTY and deterministic host replies; never opens an existing terminal."""
from collections.abc import Callable
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import termios
import time


class Pty:
    def __init__(self, argv, env, cwd, cols=120, rows=35, cell_px=None):
        self.master, self.slave = pty.openpty()
        self.original = termios.tcgetattr(self.slave)
        self.cell_px = cell_px
        pixels = (cols * cell_px[0], rows * cell_px[1]) if cell_px else (0, 0)
        assert all(0 <= value <= 65535 for value in pixels)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, *pixels))
        self.output = bytearray()
        self.arrivals = []
        self.first_byte_seconds = None
        self.started = time.monotonic()
        self._tail = b''

        def own_tty():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.child = subprocess.Popen(argv, env=env, cwd=cwd,
                                      stdin=self.slave, stdout=self.slave,
                                      stderr=self.slave, preexec_fn=own_tty,
                                      close_fds=True)

    def _answer_queries(self, data):
        # Only explicit outer-terminal queries; never forward arbitrary output.
        pending = self._tail + data
        queries = {
            # Reference herdr queries Ghostty's color scheme. Match its
            # raw_input.rs dark report; both fixture clients get dark.
            b'\x1b[?996n': b'\x1b[?997;1n',
            b'\x1b[?u': f"\x1b[?{getattr(self, 'keyboard_flags_reply', 0)}u".encode(),
            b'\x1b[c': b'\x1b[?62;22c',
            b'\x1b[5n': b'\x1b[0n',
            b'\x1b[6n': b'\x1b[1;1R',
            b'\x1b]10;?\x1b\\': b'\x1b]10;rgb:ffff/ffff/ffff\x1b\\',
            b'\x1b]11;?\x1b\\': b'\x1b]11;rgb:2828/2c2c/3434\x1b\\',
        }
        mouse_status = getattr(self, 'sgr_pixel_mouse_reply', None)
        if mouse_status is not None:
            queries[b'\x1b[?1016$p'] = f'\x1b[?1016;{mouse_status}$y'.encode()
        if self.cell_px:
            queries[b'\x1b[16t'] = f'\x1b[6;{self.cell_px[1]};{self.cell_px[0]}t'.encode()
        # Keep incomplete query prefixes; completed old queries cannot repeat.
        # Preserve wire order, especially DECRPM before primary DA. The
        # reference ends its startup probe on DA; replying in dictionary order
        # can race that terminator ahead of the requested mouse capability.
        replies = bytearray()
        while True:
            matches = [(pending.find(query), query, reply)
                       for query, reply in queries.items() if query in pending]
            if not matches:
                break
            index, query, reply = min(matches, key=lambda match: match[0])
            replies.extend(reply)
            pending = pending[:index] + pending[index + len(query):]
        if replies:
            os.write(self.master, replies)
        self._tail = b''
        for length in range(1, min(12, len(pending)) + 1):
            suffix = pending[-length:]
            if any(query.startswith(suffix) for query in queries):
                self._tail = suffix

    def read(self, seconds=0.1, answer_queries=True):
        until = time.monotonic() + seconds
        begin = len(self.output)
        while time.monotonic() < until:
            ready, _, _ = select.select([self.master], [], [], max(0, until - time.monotonic()))
            if not ready:
                break
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            if not data:
                break
            if self.first_byte_seconds is None:
                self.first_byte_seconds = time.monotonic() - self.started
            self.output.extend(data)
            self.arrivals.append((len(self.output), time.monotonic() - self.started))
            if answer_queries:
                self._answer_queries(data)
        return bytes(self.output[begin:])

    def until(self, predicate: Callable[[bytes], bool], seconds=10):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.read(min(0.05, end - time.monotonic()))
            if predicate(bytes(self.output)):
                return time.monotonic() - self.started
            if self.child.poll() is not None:
                raise RuntimeError('TTY child exited before the expected condition')
        raise TimeoutError('TTY condition deadline exceeded')

    def first_seen(self, marker):
        end = self.output.find(marker)
        if end < 0:
            return None
        end += len(marker)
        return next(seconds for offset, seconds in self.arrivals if offset >= end)

    def send(self, data):
        os.write(self.master, data)

    def resize(self, cols, rows):
        pixels = (cols * self.cell_px[0], rows * self.cell_px[1]) if self.cell_px else (0, 0)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, *pixels))
        if self.child.poll() is None:
            os.kill(self.child.pid, signal.SIGWINCH)

    def cpu_seconds(self):
        fields = Path(f'/proc/{self.child.pid}/stat').read_text().rsplit(')', 1)[1].split()
        ticks = int(fields[11]) + int(fields[12])
        return ticks / os.sysconf('SC_CLK_TCK')

    def idle_measurement(self, seconds=10):
        before_cpu = self.cpu_seconds()
        before_bytes = len(self.output)
        started = time.monotonic()
        self.read(seconds)
        elapsed = time.monotonic() - started
        return {'elapsed_seconds': elapsed,
                'client_cpu_percent': 100 * (self.cpu_seconds() - before_cpu) / elapsed,
                'output_bytes': len(self.output) - before_bytes}

    def terminal_restored(self):
        return termios.tcgetattr(self.slave) == self.original

    def close(self):
        if self.child.poll() is None:
            os.killpg(self.child.pid, signal.SIGKILL)
        self.child.wait(timeout=5)
        os.close(self.master)
        os.close(self.slave)
