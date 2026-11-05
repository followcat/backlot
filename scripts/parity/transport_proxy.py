"""Cut only an owned client's Unix transports; inspect its attach requests."""
import json
import select
import socket
import struct
import threading


class Requests:
    def __init__(self, record):
        self.record = record
        self.buffer = bytearray()
        self.plain = True
        self.done = False
        self.attach_id = None

    def feed(self, data):
        if self.done:
            return
        self.buffer.extend(data)
        if self.plain:
            if b'\n' not in self.buffer:
                assert len(self.buffer) <= 65536, "oversized plain hello"
                return
            line, rest = self.buffer.split(b'\n', 1)
            hello = json.loads(line)
            assert hello['method'] == 'hello', repr(line)
            if not hello.get('params', {}).get('framed', False):
                self.done = True  # The watcher may use plain RPC; it never attaches.
                self.buffer.clear()
                return
            self.buffer = bytearray(rest)
            self.plain = False
        while len(self.buffer) >= 9:
            channel, kind, length = struct.unpack('<IBI', self.buffer[:9])
            assert length <= 16 << 20, (channel, kind, length, bytes(self.buffer[:24]).hex())
            if len(self.buffer) < 9 + length:
                break
            payload = bytes(self.buffer[9:9 + length])
            del self.buffer[:9 + length]
            if channel == 0 and kind == 0:
                request = json.loads(payload)
                if request.get('method') == 'terminal.attach':
                    self.attach_id = request['id']
                    self.record(request['params'])
                    self.done = True
                    self.buffer.clear()
                    return


class Responses:
    def __init__(self, request, update):
        self.request = request
        self.update = update
        self.buffer = bytearray()
        self.plain = True
        self.done = False

    def feed(self, data):
        if self.done:
            return
        self.buffer.extend(data)
        if self.plain:
            if b'\n' not in self.buffer:
                assert len(self.buffer) <= 65536
                return
            line, rest = self.buffer.split(b'\n', 1)
            if not json.loads(line).get('result', {}).get('framed', False):
                self.done = True
                self.buffer.clear()
                return
            self.plain = False
            self.buffer = bytearray(rest)
        while len(self.buffer) >= 9:
            channel, kind, length = struct.unpack('<IBI', self.buffer[:9])
            assert length <= 16 << 20
            if len(self.buffer) < 9 + length:
                return
            payload = bytes(self.buffer[9:9 + length])
            del self.buffer[:9 + length]
            if channel == 0 and kind == 0:
                response = json.loads(payload)
                if response.get('id') == self.request.attach_id and 'result' in response:
                    self.update('attached', response['result'].get('channel'))
            elif channel:
                self.update('stream', (channel, kind, payload))


class Proxy:
    def __init__(self, path, target):
        self.target = target
        self.lock = threading.Lock()
        self.pairs = set()
        self.workers = []
        self.requests = []
        self.errors = []
        self.epoch = 0
        self.stopped = threading.Event()
        self.listener = socket.socket(socket.AF_UNIX)
        self.listener.bind(str(path))
        self.listener.listen()
        self.listener.settimeout(.05)
        self.thread = threading.Thread(target=self.accept)
        self.thread.start()

    def accept(self):
        while not self.stopped.is_set():
            try:
                client, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            with self.lock:
                worker = threading.Thread(target=self.bridge, args=(client, self.epoch))
                self.workers.append(worker)
                worker.start()

    def bridge(self, client, epoch):
        upstream = socket.socket(socket.AF_UNIX)
        pair = (client, upstream)
        state = {'epoch': epoch, 'success': False, 'live': False,
                 'output_bytes': 0, 'fresh_marker': False}
        tail = b''
        def record(params):
            with self.lock:
                state['params'] = params
                self.requests.append(state)
        def update(event, value):
            nonlocal tail
            with self.lock:
                if event == 'attached':
                    assert isinstance(value, int) and value > 0
                    state.update(success=True, live=True, channel=value)
                elif value[0] == state.get('channel'):
                    _, kind, payload = value
                    if kind == 2:
                        state['live'] = False
                    elif kind == 0:
                        state['output_bytes'] += len(payload)
                        tail += payload
                        state['fresh_marker'] |= b'RECOVERED STREAM' in tail
                        tail = tail[-64:]
        parser = Requests(record)
        replies = Responses(parser, update)
        try:
            upstream.connect(str(self.target))
            with self.lock:
                if epoch != self.epoch or self.stopped.is_set():
                    return
                self.pairs.add(pair)
            while not self.stopped.is_set():
                ready, _, _ = select.select(pair, [], [], .05)
                for source in ready:
                    data = source.recv(65536)
                    if not data:
                        return
                    if source is client:
                        parser.feed(data)
                    else:
                        replies.feed(data)
                    (upstream if source is client else client).sendall(data)
        except OSError:
            pass  # Cutting this owned transport intentionally interrupts IO.
        except Exception as error:
            with self.lock:
                self.errors.append(repr(error))
        finally:
            with self.lock:
                self.pairs.discard(pair)
                state['live'] = False
            for stream in pair:
                stream.close()

    def cut(self):
        with self.lock:
            self.epoch += 1
            pairs = list(self.pairs)
            for pair in pairs:
                for stream in pair:
                    try:
                        stream.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass
            return len(pairs)

    def snapshot(self):
        with self.lock:
            return [dict(r) for r in self.requests]

    def close(self):
        self.stopped.set()
        self.listener.close()
        self.cut()
        self.thread.join(timeout=2)
        assert not self.thread.is_alive()
        for worker in self.workers:
            worker.join(timeout=2)
            assert not worker.is_alive()
        assert not self.pairs and not self.errors, self.errors
