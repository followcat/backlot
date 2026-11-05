"""M3a layout scripts against two independently owned runtimes."""
import json
import shlex
import time
import tomllib
from pathlib import Path

from checks import text, verify_restored
from fixtures import Program, wait_program
from pty_host import Pty
from sessions import Core, Reference

DEFAULT = {'split_vertical':'prefix+v', 'split_horizontal':'prefix+minus',
           'reload_config':'prefix+shift+r', 'settings':'prefix+s', 'help':'prefix+?',
           'workspace_picker':'prefix+w',
           'focus_pane_left':'prefix+h', 'focus_pane_right':'prefix+l',
           'swap_pane_left':'prefix+shift+h', 'zoom':'prefix+z',
           'resize_mode':'prefix+r', 'new_tab':'prefix+c', 'rename_tab':'prefix+shift+t',
           'rename_pane':'prefix+shift+p', 'previous_tab':'prefix+p',
           'next_tab':'prefix+n', 'detach':'prefix+q',
           'cycle_pane_next':'prefix+tab', 'cycle_pane_previous':'prefix+shift+tab',
           'last_pane':'', 'move_tab_previous':'', 'move_tab_next':'', 'close_tab':'prefix+shift+x', 'close_pane':'prefix+x'}


def chord(value):
    parts=value.lower().split('+'); key=parts.pop()
    if key=='minus': key='-'
    if key=='tab':
        assert parts in ([],['shift']), 'unsupported fixture Tab modifiers'
        return b'\x1b[Z' if parts else b'\t'
    assert len(key)==1 and key.isascii(), 'fixture requires an audited ASCII binding'
    modifiers=set(parts)
    assert modifiers <= {'ctrl','alt','shift'}
    if 'shift' in modifiers: key=key.upper()
    data=bytes([ord(key.lower()) & 31]) if 'ctrl' in modifiers else key.encode()
    if 'alt' in modifiers: data=b'\x1b'+data
    assert value.lower()!='alt+f', 'never execute the unrelated configured command'
    return data


def binding(config, action):
    keys=config.get('keys',{}); value=keys.get(action,DEFAULT[action])
    if isinstance(value,list): value=value[0]
    if value.startswith('prefix+'):
        prefix=keys.get('prefix','ctrl+b')
        if isinstance(prefix,list): prefix=prefix[0]
        return chord(prefix)+chord(value[len('prefix+'):])
    return chord(value)


def inventory(owner):
    if owner.kind=='core':
        return [t['address'] for t in owner.rpc('terminal.list',{})['terminals']]
    return [p['pane_id'] for p in owner.cli('pane','list')['panes']]


def wait_count(owner,count,old=()):
    deadline=time.monotonic()+8
    while True:
        owner.ui.read(.03)
        ids=inventory(owner)
        if len(ids)==count: return next((i for i in ids if i not in old),None)
        assert time.monotonic()<deadline, 'owned layout action did not change membership'


def install(owner,name,terminal):
    program=Program(owner,name);program.terminal=terminal
    # A restarted named fixture must never replay the old control request.
    # Preserve its append-only input log so earlier leaks remain detectable.
    program.request.unlink(missing_ok=True);program.ready.unlink(missing_ok=True)
    command='exec '+shlex.join(program.argv)
    if owner.kind=='core': owner.rpc('terminal.send',{'terminal':terminal,'text':command})
    else: owner.cli('pane','run',terminal,command)
    wait_program(owner,program)
    return program


def grids(owner,programs):
    deadline=time.monotonic()+8;previous=None;since=time.monotonic()
    while True:
        owner.ui.read(.04)
        try: value=[json.loads(Path(str(p.log)+'.grid').read_text()) for p in programs]
        except (FileNotFoundError,json.JSONDecodeError): value=None
        if value!=previous: previous=value;since=time.monotonic()
        if value and time.monotonic()-since>.25: return value
        assert time.monotonic()<deadline, 'owned PTY grid readiness'


def run(args,size,binary):
    from run import checkpoint,differences,detach
    out=args.out/f'{size[0]}x{size[1]}';out.mkdir(parents=True,exist_ok=False)
    core=reference=None
    second=None
    result={'geometry':size,'checkpoints':[],'cleanup':{},'scope':'M3a','exact_M2':False}
    config=tomllib.loads(args.config.read_text())
    programs={};phase=0;foreign=None
    pending_mutation=None
    expected_input={name:b'' for name in ('alpha','beta','gamma')}
    try:
        core=Core(args.hc,args.config,out);core.start()
        alpha=Program(core,'alpha')
        terminal=core.rpc('terminal.run',{'argv':alpha.argv,'cwd':str(core.cwd),'labels':{'herdr.space':'work','herdr.tab':'main'},'no_parent':True,'no_notify':True})['address']
        alpha.terminal=terminal;wait_program(core,alpha);programs['core']=[alpha]
        foreign={'spaces':[{'id':'work','name':'work','folder':str(core.cwd),'tabs':[{'id':'main','name':'main','root':{'type':'block','content':{'terminal':terminal}}}]}]}
        core.rpc('doc.set',{'name':'herdr-app','version':0,'value':foreign})
        reference=Reference(binary,args.config,out)
        for owner,executable in [(reference,binary),(core,args.hc_tui)]:
            owner.ui=Pty([str(executable),'--session',owner.session],owner.env,owner.cwd,*size,cell_px=(10,20))
            owner.ui.keyboard_flags_reply=7;owner.ui.sgr_pixel_mouse_reply=2
            owner.initial_size=size;owner.resizes=[]
        reference.ready()
        ws=reference.cli('workspace','list')['workspaces'][0]['workspace_id']
        reference.cli('workspace','rename',ws,'work')
        tab=reference.cli('tab','list')['tabs'][0]['tab_id']
        reference.cli('tab','rename',tab,'main')
        programs['reference']=[install(reference,'alpha',inventory(reference)[0])]
        owners=(reference,core)

        def send(sequence):
            for owner in owners: owner.ui.send(sequence)
        def source_state():
            return {'layout':reference.cli('pane','layout'),'tabs':reference.cli('tab','list')}
        def mutate(sequence):
            nonlocal pending_mutation
            pending_mutation=(core.rpc('doc.get',{'name':'hc-tui'})['version'],source_state())
            send(sequence)
        def action(name):
            if name in {'split_vertical','split_horizontal','focus_pane_left','focus_pane_right','swap_pane_left','zoom','previous_tab','next_tab','close_tab','close_pane','cycle_pane_next','cycle_pane_previous','last_pane','move_tab_previous','move_tab_next'}:mutate(binding(config,name))
            else:send(binding(config,name))
        def exact_inputs():
            for owner in owners:
                for name,expected in expected_input.items():
                    path=owner.root/(name+'-input.bin')
                    actual=path.read_bytes() if path.exists() else b''
                    assert actual==expected, f'{owner.kind} {name}: unexpected program input {actual.hex()}'

        def compare(name,visible=('alpha',),required=(),paint=True,geometry=True):
            nonlocal phase,pending_mutation
            phase+=1;marker=f'phase {phase:02d}'
            current={owner.kind:[p for p in programs[owner.kind] if p.name in visible] for owner in owners}
            exact_inputs()
            if geometry and pending_mutation:
                version,previous=pending_mutation;deadline=time.monotonic()+8
                while core.rpc('doc.get',{'name':'hc-tui'})['version']<=version or source_state()==previous:
                    for owner in owners:owner.ui.read(.03)
                    assert time.monotonic()<deadline,'layout mutation did not finish on both runtimes'
            pending_mutation=None

            if geometry:
                a=grids(reference,current['reference']);b=grids(core,current['core'])
                deadline=time.monotonic()+8
                while a!=b:
                    a=grids(reference,current['reference']);b=grids(core,current['core'])
                    assert time.monotonic()<deadline, f'PTY geometry mismatch: reference {a}, candidate {b}'
                result.setdefault('pty_grids',[]).append({'name':name,'reference':a,'candidate':b})
            if paint:
                for owner in owners:
                    for p in current[owner.kind]:
                        sequence=(f'\x1b[2J\x1b[3J\x1b[Hfixture {p.name}\r\n\x1b[31mindexed\x1b[0m\r\n{marker}\x1b[4;3H').encode()
                        p.send(sequence);wait_program(owner,p,p.serial)
            frames=[]
            for owner in owners:
                deadline=time.monotonic()+8
                while True:
                    frame=checkpoint(owner,name,size,args.decoder);rendered=text(frame)
                    ready=all(s in rendered for s in required)
                    if paint: ready=ready and rendered.count(marker)>=len(current[owner.kind])
                    if ready and owner.capture_quiet: break
                    assert time.monotonic()<deadline, f'{owner.kind} {name} did not reach expected paint'
                frames.append(frame)
            exact_inputs()
            if geometry:
                after_a=grids(reference,current['reference']);after_b=grids(core,current['core'])
                assert after_a==a and after_b==b, 'PTY geometry changed during capture; checkpoint is invalid'
            diff=differences(*frames,args.default_title)
            (out/(name+'-diff.json')).write_text(json.dumps(diff,ensure_ascii=False,indent=2)+'\n')
            bugs=[entry for entry in diff if 'adaptation' not in entry]
            result['checkpoints'].append({'name':name,'different_cells':len(bugs),'status':'fail' if bugs else 'pass','data_adaptations':[d for d in diff if 'adaptation' in d]})
            print(json.dumps({'geometry':size,'checkpoint':name,'differences':len(bugs)}),flush=True)
            return frames

        compare('structure-baseline')  # Also proves frontend readiness before UI keys.
        action('split_vertical')
        for owner in owners:
            new=wait_count(owner,2,[p.terminal for p in programs[owner.kind]])
            programs[owner.kind].append(install(owner,'beta',new))
        compare('split-right',('alpha','beta'))
        for owner in owners:
            for p in programs[owner.kind]:
                p.send(b'\x1b[5 q\x1b]12;#123456\x07\x1b]4;1;#345678\x07');wait_program(owner,p,p.serial)
        compare('one-shot-appearance',('alpha','beta'))
        action('focus_pane_left');compare('focus-left',('alpha','beta'))
        action('focus_pane_right');compare('focus-right',('alpha','beta'))
        action('cycle_pane_next');compare('cycle-next',('alpha','beta'))
        action('cycle_pane_previous');compare('cycle-previous',('alpha','beta'))
        if config.get('keys',{}).get('last_pane'):
            action('last_pane');compare('last-pane',('alpha','beta'))
            action('last_pane');compare('last-pane-return',('alpha','beta'))
        action('swap_pane_left');compare('swap-left',('alpha','beta'))
        action('resize_mode');compare('resize-mode',('alpha','beta'),required=('RESIZE',))
        mutate(b'l');send(b'\x1b[27u');compare('resize-right',('alpha','beta'))
        action('zoom');compare('zoom',('beta',))
        action('zoom');compare('unzoom',('alpha','beta'))
        action('rename_pane');compare('rename-pane-dialog',('alpha','beta'),required=('rename pane',),paint=False,geometry=False)
        send(b'pane-b\r');compare('renamed-pane',('alpha','beta'),required=('pane-b',))
        action('new_tab');compare('new-tab-dialog',('alpha','beta'),required=('new tab',),paint=False,geometry=False)
        send(b'next\r')
        for owner in owners:
            new=wait_count(owner,3,[p.terminal for p in programs[owner.kind]])
            programs[owner.kind].append(install(owner,'gamma',new))
        compare('new-tab',('gamma',))
        action('previous_tab')
        retained=compare('lru-return',('alpha','beta'),required=('fixture alpha','fixture beta'),paint=False)
        assert all(frame['cursor']['shape']=='Bar' for frame in retained), 'one-shot cursor was lost'
        result['one_shot_idle_return']=True
        action('next_tab');compare('next-tab',('gamma',))
        action('rename_tab');compare('rename-tab-dialog',('gamma',),required=('rename tab',),paint=False,geometry=False)
        send(b'\x03renamed\r');tab_frames=compare('renamed-tab',('gamma',))
        for move in ('move_tab_previous','move_tab_next'):
            if config.get('keys',{}).get(move):
                action(move);tab_frames=compare(move.replace('_','-'),('gamma',))
        if size[0]>64:
            rows=[(row['y'],''.join(c['text'] for c in row['cells'])) for row in tab_frames[0]['cells']]
            row,line=next((y,line) for y,line in rows if 'main' in line and 'renamed' in line)
            start=line.index('renamed')+2
            end=line.index('main')
            if start<end:end=min(size[0]-1,end+len('main')+4)
            mutate(f'\x1b[<0;{start+1};{row+1}M\x1b[<32;{end+1};{row+1}M\x1b[<0;{end+1};{row+1}m'.encode())
            compare('tab-drag',('gamma',))
        send(b'\x07');compare('lock',('gamma',),required=('LOCKED',))
        prefix=config.get('keys',{}).get('prefix','ctrl+b')
        if isinstance(prefix,list):prefix=prefix[0]
        locked_bytes=chord(prefix)+b'c'
        expected_input['gamma']=locked_bytes
        send(locked_bytes)
        for owner in owners:
            p=programs[owner.kind][-1];deadline=time.monotonic()+5
            while not p.received()==locked_bytes:
                owner.ui.read(.03);assert time.monotonic()<deadline,'locked prefix not forwarded'
            assert len(inventory(owner))==3
        compare('locked-prefix',('gamma',))
        send(b'\x07');compare('unlock',('gamma',))
        # Two real hc-tui processes, same owned core. Fake-core tests force
        # conflicts deterministically; here simultaneous edits must converge.
        from types import SimpleNamespace
        second=SimpleNamespace(kind='core-second',out=out,initial_size=size,resizes=[])
        second.ui=Pty([str(args.hc_tui),'--session',core.session],core.env,core.cwd,*size,cell_px=(10,20))
        second.ui.keyboard_flags_reply=7;second.ui.sgr_pixel_mouse_reply=2
        deadline=time.monotonic()+8
        while 'fixture gamma' not in text(checkpoint(second,'second-ready',size,args.decoder)):
            assert time.monotonic()<deadline,'second client readiness'
        before=core.rpc('doc.get',{'name':'hc-tui'})['version']
        for owner in (core,second):owner.ui.send(binding(config,'rename_tab'))
        for owner in (core,second):
            deadline=time.monotonic()+5
            while 'rename tab' not in text(checkpoint(owner,'shared-dialog',size,args.decoder)):
                assert time.monotonic()<deadline,'shared dialog readiness'
        core.ui.send(b'\x03shared-a\r');second.ui.send(b'\x03shared-b\r')
        deadline=time.monotonic()+8
        while True:
            stored=core.rpc('doc.get',{'name':'hc-tui'})
            labels=[t.get('name') for t in stored['value']['tabs'].values()]
            chosen=next((v for v in labels if v in ('shared-a','shared-b')),None)
            a=checkpoint(core,'shared-final',size,args.decoder)
            b=checkpoint(second,'shared-final',size,args.decoder)
            if stored['version']>=before+2 and chosen and chosen in text(a) and chosen in text(b) and not differences(a,b):break
            assert time.monotonic()<deadline,'two clients failed to converge'
        assert stored['version']>=before+2,'both concurrent intentions must commit'
        result['two_clients']={'passed':True,'version_before':before,'version_after':stored['version'],'final_label':chosen,'cell_differences':0,'forced_conflict':'fake-core unit test'}
        detach(second,args.decoder,binding(config,'detach'));second.ui.close();second=None
        core.ui.send(binding(config,'rename_tab'));core.ui.read(.2);core.ui.send(b'\x03renamed\r')
        compare('shared-return',('gamma',))

        action('close_tab')
        for owner in owners: wait_count(owner,2)
        final_frames=compare('close-tab',('alpha','beta'))
        for owner in owners:programs[owner.kind]=[p for p in programs[owner.kind] if p.name!='gamma']
        # Derive the hit from the independent reference frame and actual PTY
        # geometry, never from the candidate's rendering or expected cells.
        rows=[(row['y'],''.join(c['text'] for c in row['cells'])) for row in final_frames[0]['cells']]
        y,line=next((y,line) for y,line in rows if 'fixture beta' in line)
        beta=next(p for p in programs['reference'] if p.name=='beta')
        cols=json.loads(Path(str(beta.log)+'.grid').read_text())[0]
        seam=line.index('fixture beta')-1+cols+3
        target=seam+3
        mutate(f'\x1b[<0;{seam};{y+2}M\x1b[<32;{target};{y+2}M\x1b[<0;{target};{y+2}m'.encode())
        compare('split-drag',('alpha','beta'))
        action('split_horizontal')
        for owner in owners:
            new=wait_count(owner,3,[p.terminal for p in programs[owner.kind]])
            programs[owner.kind].append(install(owner,'gamma',new))
        compare('split-down-three',('alpha','beta','gamma'))
        action('close_pane')
        for owner in owners:
            wait_count(owner,2)
            programs[owner.kind]=[p for p in programs[owner.kind] if p.name!='gamma']
        compare('close-pane',('alpha','beta'))
        action('close_pane')
        for owner in owners:
            wait_count(owner,1)
            programs[owner.kind]=[p for p in programs[owner.kind] if p.name!='beta']
        compare('alone-after-close',('alpha',))
        action('close_pane');compare('close-confirm',('alpha',),required=('Close workspace?',),paint=False,geometry=False)
        send(b'\x1b[27u')
        compare('close-cancel-navigate',('alpha',),required=(('NAVIGATE' if size[0]>64 else 'switch'),),paint=False,geometry=False)
        send(b'\x1b[27u');compare('close-cancel-return',('alpha',))
        from configuration import run as config_round
        config_round(owners, config, compare, size, args.decoder, out, result)
        assert core.rpc('doc.get',{'name':'herdr-app'})['value']==foreign, 'client wrote the foreign document'
        for owner in owners:
            restored=detach(owner,args.decoder,binding(config,'detach'));verify_restored(restored)
        result['termios_restored']=True
        result['passed']=all(c['status']=='pass' for c in result['checkpoints'])
    except BaseException as error:
        result['error']=repr(error)
        for owner in (reference,core):
            if owner is None or owner.ui is None:continue
            try:
                owner.ui.read(.1)
                (out/(owner.kind+'-failure.raw')).write_bytes(owner.ui.output)
                state={p.name:{'expected_serial':p.serial,'ready':p.ready.read_text() if p.ready.exists() else None} for p in programs.get(owner.kind,[])}
                (out/(owner.kind+'-failure.json')).write_text(json.dumps(state,indent=2)+'\n')
            except (OSError,ValueError):pass
        raise
    finally:
        errors=[]
        if second is not None:second.ui.close()
        for owner in [core,reference]:
            if owner is None:continue
            try:owner.close();result['cleanup'][owner.kind]='pass'
            except BaseException as error:owner.recovery();result['cleanup'][owner.kind]=repr(error);errors.append(error)
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        if errors:raise RuntimeError('owned cleanup failed; retain recovery records') from errors[0]
    return result
