#!/usr/bin/env python3
"""Exercise TUI keys with isolated projects. Unix uses stdlib PTY; Windows uses pywinpty."""
import argparse
import codecs
import os
import re
import select
import struct
import subprocess
import sys
import tempfile
import time
import unicodedata
from pathlib import Path

if os.name == 'nt':
    try:
        from winpty import PtyProcess
    except ImportError:
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'target/ui-check/python'))
        from winpty import PtyProcess
else:
    import fcntl
    import pty
    import termios

class WindowsProcess:
    def __init__(self, process): self.pty = process
    def poll(self): return None if self.pty.isalive() else self.pty.exitstatus
    def wait(self, timeout=5):
        end = time.monotonic() + timeout
        while self.pty.isalive() and time.monotonic() < end: time.sleep(.02)
        if self.pty.isalive(): raise TimeoutError('CLI did not exit')
        return self.pty.exitstatus
    def kill(self): self.pty.terminate(force=True)

ESC = b'\x1b'
HOME = b'\x1b[H'
ENTER = b'\r'

class Terminal:
    def __init__(self, command, width=40, env=None):
        self.width, self.height = width, 24
        self.cells = [[' '] * width for _ in range(self.height)]
        self.x = self.y = 0
        self.pending = ''
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')
        terminal_env = {**os.environ, "TERM":"xterm-256color", **(env or {})}
        self.winpty = None
        if os.name == 'nt':
            self.winpty = PtyProcess.spawn(command, dimensions=(self.height, width), env=terminal_env)
            self.process = WindowsProcess(self.winpty)
            self.master = self.winpty.fileobj
        else:
            self.master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', self.height, width, 0, 0))
            self.process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave,
                                            env=terminal_env, start_new_session=True)
            os.close(slave)
        self.raw = bytearray()
        self.pump(.3)

    @property
    def screen(self):
        return '\n'.join(''.join(row) for row in self.cells)

    def feed(self, data):
        self.raw.extend(data)
        if b'\x1b[6n' in data:
            self.write(b'\x1b[1;1R')
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith('\x1b['):
                match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', self.pending)
                if not match:
                    break
                self.pending = self.pending[match.end():]
                raw, _, operation = match.groups()
                args = [int(a) if a else 1 for a in raw.lstrip('?').split(';')]
                if operation in 'Hf':
                    self.y = max(0, min(self.height-1, args[0]-1))
                    self.x = max(0, min(self.width-1, (args[1] if len(args)>1 else 1)-1))
                elif operation == 'J' and raw in ('2', '3') or operation == 'h' and raw == '?1049':
                    self.cells = [[' '] * self.width for _ in range(self.height)]
                    self.x = self.y = 0
                elif operation == 'K':
                    self.cells[self.y][self.x:] = [' '] * (self.width-self.x)
                elif operation == 'G': self.x = max(0, args[0]-1)
                elif operation == 'A': self.y = max(0, self.y-args[0])
                elif operation == 'B': self.y = min(self.height-1, self.y+args[0])
                elif operation == 'C': self.x = min(self.width-1, self.x+args[0])
                elif operation == 'D': self.x = max(0, self.x-args[0])
                continue
            if self.pending.startswith('\x1b'):
                if len(self.pending)<2: break
                self.pending = self.pending[2:]
                continue
            char, self.pending = self.pending[0], self.pending[1:]
            if char == '\r': self.x = 0
            elif char == '\n': self.y = min(self.height-1, self.y+1)
            elif char >= ' ' and not unicodedata.combining(char):
                width = 2 if unicodedata.east_asian_width(char) in ('W','F') else 1
                if self.x < self.width:
                    self.cells[self.y][self.x] = char
                    if width == 2 and self.x+1 < self.width: self.cells[self.y][self.x+1] = ''
                self.x += width

    def pump(self, seconds=.15):
        end = time.monotonic()+seconds
        while time.monotonic()<end:
            if select.select([self.master], [], [], min(.03, max(0,end-time.monotonic())))[0]:
                try: data = self.winpty.read(65536).encode() if self.winpty else os.read(self.master, 65536)
                except (OSError, EOFError): break
                if not data: break
                self.feed(data)

    def write(self, data):
        if self.winpty: self.winpty.write(data if isinstance(data,str) else data.decode())
        else: os.write(self.master, data.encode() if isinstance(data,str) else data)

    def send(self, data):
        self.write(data)
        self.pump(.35)

    def resize(self, width, height=24):
        self.width, self.height = width, height
        self.cells = [[' '] * width for _ in range(height)]
        self.x = self.y = 0
        if self.winpty: self.winpty.setwinsize(height,width)
        else: fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH',height,width,0,0))
        self.pump(.4)

    def expect(self, text):
        end = time.monotonic()+4
        while time.monotonic()<end:
            if text in self.screen: return
            self.pump(.1)
        raise AssertionError(f'Missing {text!r}:\n{self.screen}')

    def choose(self, number): self.send(str(number).encode()+ENTER)

    def back(self): self.send(ESC)

    def home(self): self.send(HOME)

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait()
        if self.winpty: self.winpty.close()
        else: os.close(self.master)


def check(binary):
    with tempfile.TemporaryDirectory(prefix='novel-nav-') as directory:
        data, project = Path(directory)/'data', Path(directory)/'测试项目'
        base = [str(binary), '--data-dir', str(data)]
        def cli(*args):
            return subprocess.run(base+list(args), check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        cli('project', 'create', str(project))
        cli('chapter', 'new', 'ch001')
        chapter = project/'chapters/ch001/chapter.md'
        chapter.write_text('旧稿\n' + '山中的来信，终于送到了门前。\n'*30, encoding='utf-8')
        term = Terminal(base+['tui'], width=120)
        try:
            term.expect('进入章节工作台')
            term.back(); term.expect('进入章节工作台')
            assert term.process.poll() is None
            term.choose(1); term.expect('ch001'); term.choose(1)
            term.expect('旧稿'); term.expect('编辑正文')
            # The wide workspace displays navigation, directory and manuscript.
            assert '导航' in term.screen and '章节目录' in term.screen
            snapshots = Path('target/ui-check')
            snapshots.mkdir(parents=True,exist_ok=True)
            (snapshots/'cli-wide.txt').write_text(term.screen,encoding='utf-8')
            for width in [94,40,120]:
                term.resize(width);term.expect('旧稿')
            term.send(b'\x1b[B'); term.send(b'\x1b[B')
            term.send('e'); term.send('未保存'); term.home();term.expect('回主页前保存？')
            term.send('c');term.expect('Ctrl+S / F2 保存')
            term.back();term.expect('返回前保存？');term.send('d');term.expect('编辑正文')
            assert chapter.read_text(encoding='utf-8').startswith('旧稿')
            term.send('e');term.send(b'\x01');term.send('首');term.send(b'\x13')
            term.expect('编辑正文');assert '首' in chapter.read_text(encoding='utf-8')
            term.send('e');term.send(b'\x06');term.send('旧稿');term.back();term.expect('Ctrl+S / F2 保存')
            chapter.write_text('外部新版',encoding='utf-8')
            term.send('冲突稿');term.send(b'\x13');term.expect('保存失败')
            assert chapter.read_text(encoding='utf-8')=='外部新版'
            term.back();term.send('d');term.expect('编辑正文')
            # Generation, preview and manual mode retain independent per-chapter drafts.
            term.send('g');term.expect('编辑事件骨架');term.choose(2)
            term.send('进入山谷');term.send(b'\x13');term.expect('事件骨架')
            term.choose(7);term.expect('高级生成选项');term.choose(1) # disable memory
            term.back();term.choose(5);term.expect('编辑系统提示词')
            assert not list((data/'jobs').glob('*/request.json'))
            term.choose(2);term.send(b'\x01');term.send('本章系统：');term.send(b'\x13')
            term.back();term.expect('已采用本章预览')
            term.choose(2);term.send('变化');term.send(b'\x13')
            term.choose(6);term.expect('本章手动预览已过期');term.back()
            assert not list((data/'jobs').glob('*/request.json'))
            term.choose(1);term.expect('手动完整提示词')
            term.choose(2);term.send('手动草稿');term.send(b'\x13')
            term.choose(1);term.expect('事件骨架');term.choose(1)
            term.choose(2);term.expect('手动草稿');term.back()
            term.home();term.expect('继续当前章节')
            term.choose(1);term.expect('外部新版')
            # Global navigation is reachable from the workbench without walking back.
            term.send(b'\x1bOP') # F1
            term.send(b'\x1b[B');term.send(b'\x1b[B');term.send(ENTER)
            term.expect('作者资料');term.expect('故事设定')
            term.home();term.expect('继续当前章节')
            term.choose(7);assert term.process.wait(timeout=5)==0
            raw=term.raw
            # One alternate-screen lifetime across menus, readers and editors.
            assert raw.count(b'\x1b[?1049h')==1, 'Menus repeatedly re-entered alternate screen'
            assert b'\x1b[?1049l' in raw
        finally: term.close()
        # Search, module popup and editor protection on phone-sized terminals.
        term=Terminal(base+['--no-color','tui'],width=40)
        try:
            term.expect('进入章节工作台');term.choose(1)
            term.send('/missing');term.send(ENTER);term.expect('没有匹配项')
            term.back();term.expect('ch001');term.choose(1);term.expect('外部新版')
            (Path('target/ui-check')/'cli-narrow.txt').write_text(term.screen,encoding='utf-8')
            term.send(b'\x1bOP');term.expect('模块导航');term.back()
            term.send('e');term.send('窄屏稿');term.home();term.expect('回主页前保存？')
            term.send('d');term.expect('继续当前章节');term.choose(7)
            assert term.process.wait(timeout=5)==0
        finally:term.close()
        # External editor suspends the TUI and restores it after the save.
        editor=Path(directory)/'external-editor.py'
        editor.write_text("import sys\nfrom pathlib import Path\np=Path(sys.argv[1]);p.write_text(p.read_text(encoding='utf-8')+' external edit',encoding='utf-8')\n",encoding='utf-8')
        editor_command='"'+Path(sys.executable).as_posix()+'" "'+editor.as_posix()+'"'
        term=Terminal(base+['tui'],width=100,env={'VISUAL':editor_command,'NO_COLOR':'1'})
        try:
            term.expect('进入章节工作台');term.choose(1);term.choose(1);term.choose(9)
            term.expect('external edit')
            assert chapter.read_text(encoding='utf-8').endswith('external edit')
            # Secret editing never writes the entered value into terminal output.
            term.home();term.send(b'\x1bOP')
            for _ in range(5):term.send(b'\x1b[B')
            term.send(ENTER);term.expect('模型连接方式');term.choose(3)
            term.send('fixture-secret-123');term.send(ENTER);term.expect('密钥已保存')
            assert b'fixture-secret-123' not in term.raw
            term.back();term.home();term.expect('继续当前章节');term.choose(7)
            assert term.process.wait(timeout=5)==0
            assert term.raw.count(b'\x1b[?1049h')==2 # startup and resume after external editor
        finally:term.close()
        # A completed task's reader can navigate to a module without losing its target.
        import json, uuid
        job_id=str(uuid.uuid4()); job=data/'jobs'/job_id;job.mkdir(parents=True)
        (job/'status.json').write_text(json.dumps({'id':job_id,'project':str(project),'state':'succeeded','stage':'完成','created_at':'2026-11-11T09:00:00Z','updated_at':'2026-11-11T09:01:00Z','result':None,'error':None}),encoding='utf-8')
        (job/'request.json').write_text(json.dumps({'project':str(project),'settings':{},'action':{'kind':'check','id':'ch001'}}),encoding='utf-8')
        term=Terminal(base+['tui'],width=100)
        try:
            term.expect('进入章节工作台');term.send(b'\x1bOP')
            for _ in range(7):term.send(b'\x1b[B')
            term.send(ENTER);term.expect('已完成');term.choose(1);term.expect('任务状态')
            term.choose(1);term.expect('external edit')
            term.send(b'\x1bOP');term.expect('模块导航');term.choose(4)
            term.expect('作者资料');term.home();term.expect('进入章节工作台');term.choose(6)
            assert term.process.wait(timeout=5)==0
        finally:term.close()
        # Plain mode also requires an interactive terminal; no ANSI screen control.
        term=Terminal(base+['--plain'],width=80)
        try:
            term.expect('进入章节工作台')
            for line in ['1','1','0','h','7']: term.send(line+ENTER.decode())
            assert term.process.wait(timeout=5)==0
            output=term.raw.decode('utf-8',errors='replace')
            assert '进入章节工作台' in output and '编辑正文' in output
            assert b'\x1b[?1049h' not in term.raw
        finally: term.close()
        no_tty=subprocess.run(base+['tui'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5)
        assert no_tty.returncode!=0 and '终端' in no_tty.stderr.decode('utf-8')
    print('TUI navigation passed: wide/narrow layouts, search, global navigation, editor conflicts, previews, manual drafts, plain mode, TTY and screen restoration.')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=Path('target/debug/novel.exe' if os.name == 'nt' else 'target/debug/novel'))
    check(parser.parse_args().binary.resolve())
