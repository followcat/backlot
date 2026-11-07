"""Neutral child programs; all control files belong to the scratch root."""
import base64
import json


GEOMETRY = r'''
class Geometry:
 def __init__(self,path):
  self.path=path
  self.published=None
 def sample(self):
  grid=tuple(os.get_terminal_size(0))
  if grid!=self.published:
   with open(self.path+'.next','w') as f:json.dump(list(grid),f)
   os.replace(self.path+'.next',self.path)
   self.published=grid
  return grid
 def ready(self,expected):
  actual=tuple(os.get_terminal_size(0))
  return actual==self.published and (expected is None or tuple(expected)==self.published)
'''

PROGRAM = r'''
import json,os,sys,threading,time,tty
tty.setraw(0)
name,request,ready,log=sys.argv[1:]
received=os.open(log,os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600)
''' + GEOMETRY + r'''
geometry=Geometry(log+'.grid')
geometry.sample()
initial=("\x1b[2J\x1b[3J\x1b[Hfixture "+name+"\r\n"
 "\x1b[31;4mred-under\x1b[0m\r\n"
 "\x1b[38;2;12;34;56;58;2;65;43;21;4:3mcurly\x1b[0m \x1b[4:2mdouble\x1b[0m "
 "\x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m\r\n"
 "\x1b[31;44;7minverse\x1b[0m \x1b[31;44;8mhidden\x1b[0m\r\n"
 "\x1b[1;2;3;9mbold-dim-italic-strike\x1b[0m\r\n"
 "中 e\u0301 👩‍💻\r\n"
 "\x1b[16;2HPHASE initial\x1b[18;8H"
 "\x1b]2;fixture-"+name+"\x1b\\")
os.write(1,initial.encode())
open(ready,'w').write('initial')
def control():
 last=-1
 while True:
  geometry.sample()
  try:
   with open(request) as f:data=json.load(f)
  except (FileNotFoundError,json.JSONDecodeError):
   time.sleep(.01);continue
  if data['id']!=last:
   if not geometry.ready(data.get('grid')):
    time.sleep(.01);continue
   os.write(1,bytes.fromhex(data['hex']))
   last=data['id']
   with open(ready,'w') as f:f.write(str(last))
  time.sleep(.01)
threading.Thread(target=control,daemon=True).start()
all_input=bytearray()
while True:
 data=os.read(0,8192)
 if not data:break
 os.write(received,data)
 all_input.extend(data)
 summary=all_input.hex()[-42:]
 os.write(1,("\x1b[19;2HINPUT "+summary+"\x1b[K\x1b[18;8H").encode())
'''


class Program:
    def __init__(self, owner, name):
        assert name in ('alpha', 'beta', 'gamma')
        self.owner = owner
        self.name = name
        self.request = owner.root / (name + '-request.json')
        self.ready = owner.root / (name + '-ready')
        self.log = owner.root / (name + '-input.bin')
        self.argv = ['/usr/bin/python3', '-u', '-c', PROGRAM, name,
                     str(self.request), str(self.ready), str(self.log)]
        self.serial = 0
        self.terminal = None
        self.workspace = None

    def send(self, sequence, grid=None):
        self.serial += 1
        temporary = self.request.with_suffix('.next')
        message = {'id': self.serial, 'hex': sequence.hex()}
        if grid is not None:
            message['grid'] = grid
        temporary.write_text(json.dumps(message))
        temporary.replace(self.request)

    def received(self):
        return self.log.read_bytes() if self.log.exists() else b''

    def status(self, state):
        assert state in ('working', 'blocked', 'idle')
        self.report(state)

    def report(self, state, record=''):
        """OSC 7501 word: readiness aliases (`done`) and named records."""
        assert state in ('working', 'blocked', 'idle', 'done')
        title = base64.b64encode(('task-' + self.name).encode()).decode()
        target = f'id={record}:' if record else ''
        sequence = f'\x1b]7501;{target}state={state}:app=fixture:title={title}\x1b\\'.encode()
        self.send(sequence)


def wait_program(owner, program, serial='initial'):
    import time
    deadline = time.monotonic() + 10
    while not program.ready.exists() or program.ready.read_text() != str(serial):
        assert time.monotonic() < deadline, 'owned fixture readiness'
        if owner.ui:
            owner.ui.read(.02)
        else:
            time.sleep(.02)


def prepare_core(owner, paths=None, labelled=True):
    programs = []
    for name in ('alpha', 'beta'):
        program = Program(owner, name)
        result = owner.rpc('terminal.run', {'argv': program.argv, 'cwd': str(paths[name] if paths else owner.cwd),
            'labels': {'herdr.space': name} if labelled else {}, 'no_notify': True, 'no_parent': True})
        program.terminal = result['address']
        wait_program(owner, program)
        programs.append(program)
    return programs


def prepare_reference(owner, paths=None, labelled=True):
    import shlex
    workspaces = owner.cli('workspace', 'list')['workspaces']
    assert len(workspaces) == 1, 'new owned reference must have one initial workspace'
    initial = workspaces[0]['workspace_id']
    panes = owner.cli('pane', 'list')['panes']
    assert len(panes) == 1, 'new owned reference must have one initial pane'
    programs = []
    for name in ('alpha', 'beta'):
        program = Program(owner, name)
        if name == 'alpha':
            if labelled:
                owner.cli('workspace', 'rename', initial, name)
            program.workspace = initial
            program.terminal = panes[0]['pane_id']
        else:
            arguments = ['workspace', 'create', '--cwd', str(paths[name] if paths else owner.cwd), '--no-focus']
            if labelled:
                arguments.extend(['--label', name])
            result = owner.cli(*arguments)
            program.workspace = result['workspace']['workspace_id']
            program.terminal = result['root_pane']['pane_id']
        owner.cli('pane', 'run', program.terminal, 'exec ' + shlex.join(program.argv))
        wait_program(owner, program)
        programs.append(program)
    owner.cli('workspace', 'focus', programs[0].workspace)
    return programs
