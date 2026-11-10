#!/usr/bin/env python3
"""Exercise real TUI key events in an isolated Unix PTY. Uses only Python's standard library."""
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
    print('PTY navigation checks run on macOS/Linux; Windows still runs Rust CLI tests.')
    sys.exit(0)
import fcntl
import pty
import termios

ESC = b'\x1b'
HOME = b'\x1b[H'
ENTER = b'\r'

class Terminal:
    def __init__(self, command, width=40):
        self.width, self.height = width, 24
        self.cells = [[' '] * width for _ in range(self.height)]
        self.x = self.y = 0
        self.pending = ''
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', self.height, width, 0, 0))
        self.process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave,
                                        env={**os.environ, 'TERM': 'xterm-256color'}, start_new_session=True)
        os.close(slave)
        self.raw = bytearray()
        self.pump(.3)

    @property
    def screen(self):
        return '\n'.join(''.join(row) for row in self.cells)

    def feed(self, data):
        self.raw.extend(data)
        if b'\x1b[6n' in data:
            os.write(self.master, b'\x1b[1;1R')
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
                try: data = os.read(self.master, 65536)
                except OSError: break
                if not data: break
                self.feed(data)

    def send(self, data):
        os.write(self.master, data.encode() if isinstance(data,str) else data)
        self.pump(.35)

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
        os.close(self.master)


def check(binary):
    with tempfile.TemporaryDirectory(prefix='novel-nav-') as directory:
        data, project = Path(directory)/'data', Path(directory)/'测试项目'
        base = [str(binary), '--data-dir', str(data)]
        def cli(*args):
            subprocess.run(base+list(args), check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        cli('project', 'create', str(project))
        cli('chapter', 'new', 'ch001')
        chapter = project/'chapters/ch001/chapter.md'
        chapter.write_text('旧稿', encoding='utf-8')
        term = Terminal(base+['tui'])
        try:
            term.expect('章节工作台')
            term.back()
            term.expect('Esc 已在主页')
            assert term.process.poll() is None, 'Esc on homepage must not exit'
            term.choose(2);term.expect('章节工作台')
            term.choose(1);term.expect('阅读正文')
            term.choose(1);term.expect('旧稿')
            term.back();term.expect('编辑正文')
            term.back();term.expect('输入章号 / 新章节')
            # Visible back button is equivalent to Esc.
            term.choose(3);term.expect('Esc 已在主页')
            # Dirty editor: continue, discard, save, and save-before-home.
            term.choose(2);term.choose(1);term.choose(2)
            term.send('未保存');term.home();term.expect('回主页前保存？')
            term.send('c');term.expect('Ctrl+S / F2 保存')
            term.back();term.expect('返回前保存？');term.send('d');term.expect('阅读正文')
            assert chapter.read_text(encoding='utf-8') == '旧稿'
            term.choose(2);term.send(b'\x01');term.send('首');term.send(b'\x05');term.send('尾');term.send(b'\x13')
            term.expect('阅读正文');assert chapter.read_text(encoding='utf-8') == '首旧稿尾'
            term.choose(2);term.send('保存后主页');term.home();term.expect('回主页前保存？');term.send('s');term.expect('Esc 已在主页')
            assert chapter.read_text(encoding='utf-8').endswith('保存后主页')
            # Search closes before the editor does; a conflict must retain the editor buffer.
            term.choose(2);term.choose(1);term.choose(2)
            term.send(b'\x06');term.send('旧稿');term.back();term.expect('Ctrl+S / F2 保存')
            chapter.write_text('外部新版', encoding='utf-8')
            term.send('冲突稿');term.send(b'\x13');term.expect('保存失败')
            assert chapter.read_text(encoding='utf-8') == '外部新版'
            term.back();term.send('d');term.expect('阅读正文')
            # A failed command returns to the same chapter's action menu.
            term.choose(4);term.expect('内容冲突');term.back();term.expect('阅读正文')
            # Generation drafts survive Home and re-entry without submitting a model request.
            term.choose(5);term.choose(1);term.send('生成草稿');term.send(b'\x13');term.expect('生成设置')
            term.home();term.choose(2);term.choose(1);term.choose(5);term.choose(1);term.expect('生成草稿')
            term.back();term.expect('生成设置')
            # The generation page previews its own draft without submitting a worker or touching prose.
            term.choose(3)  # Disable context so the short prompt fits the terminal viewport.
            before_preview = chapter.read_text(encoding='utf-8')
            term.choose(9);term.expect('编辑系统提示词');term.choose(1)
            term.expect('系统提示词');term.expect('章节提示词');term.expect('生成草稿')
            assert chapter.read_text(encoding='utf-8') == before_preview
            assert not list((data/'jobs').glob('*/request.json')), 'Preview must not submit jobs'
            term.back();term.expect('编辑章节提示词')
            term.back();term.expect('提交后台生成')
            term.back();term.expect('阅读正文');term.home()
            # Memory, project, report, and empty job screens all return by one level.
            term.choose(3);term.choose(1);term.choose(1);term.back();term.expect('内置编辑器')
            term.back();term.expect('memory/story_bible.yaml');term.home()
            term.choose(1);term.choose(1);term.back();term.expect('按路径打开');term.home()
            term.choose(6);term.choose(1);term.choose(1);term.back();term.expect('一致性报告')
            term.back();term.expect('输入章号 / 新章节');term.home()
            term.choose(7);term.expect('返回上一级');term.back();term.expect('Esc 已在主页')
            term.choose(9);term.pump(.2);assert term.process.wait(timeout=3) == 0
            assert b'\x1b[?1049l' in term.raw
        finally: term.close()
        prompt_file = project/'.prompt-settings.json'
        prompt_before = prompt_file.read_bytes() if prompt_file.exists() else None
        # Per-chapter scheme and editable previews remain local; stale edits cannot submit a job.
        term = Terminal(base+['tui'], width=80)
        try:
            term.expect('章节工作台')
            term.choose(2);term.choose(1);term.choose(5)
            term.choose(1);term.send('进入山谷');term.send(b'\x13')
            term.choose(3)  # Disable context.
            term.choose(11);term.expect('跟随项目默认');term.choose(3)
            term.expect('本章生成方案：精简四段式')
            import json
            current = json.loads(subprocess.check_output(base+['prompt','preset']))
            assert current['generation_preset'] == 'general', 'Chapter selection changed the project default'
            term.choose(9);term.expect('编辑章节提示词')
            term.choose(2);term.send(b'\x01');term.send('系统标记：');term.send(b'\x13')
            term.expect('编辑章节提示词')
            term.choose(3);term.send(b'\x01');term.send('正文标记：');term.send(b'\x13')
            term.expect('查看完整提示词')
            term.choose(1);term.expect('系统标记：');term.send(b'\x1b[6~');term.expect('正文标记：')
            term.back();term.back();term.expect('已采用本章预览')
            assert (prompt_file.read_bytes() if prompt_file.exists() else None) == prompt_before, 'Local preview wrote project templates'
            # Reopening must preserve the adopted edits.
            term.choose(9);term.choose(1);term.expect('系统标记：');term.back();term.back()
            # Editing the outline invalidates the adoption before any overwrite confirmation or worker.
            term.choose(1);term.send('变化');term.send(b'\x13')
            term.choose(10);term.expect('本章手动预览已过期');term.back()
            assert not list((data/'jobs').glob('*/request.json'))
            # A fresh preview follows the updated input; reset clears the previous local edits.
            term.choose(9);term.choose(1);term.expect('变化');term.back();term.choose(5)
            term.choose(1);term.expect('系统提示词')
            assert '系统标记：' not in term.screen
            term.back();term.back();term.expect('当前自动');term.home()
            term.choose(9);assert term.process.wait(timeout=3) == 0
        finally: term.close()
        # Project templates are editable as plain text, independently from chapter previews.
        import json
        templates_before = json.loads(subprocess.check_output(base+['prompt','show']))
        term = Terminal(base+['tui'], width=80)
        try:
            term.expect('章节工作台')
            term.choose(4);term.expect('编辑提示词模板（分类）')
            term.choose(2);term.expect('精简四段式');term.choose(2)
            term.expect('编辑任务提示词模板')
            term.choose(1);term.send(b'\x01');term.send('永久系统标记：');term.send(b'\x13')
            term.expect('编辑任务提示词模板')
            term.choose(2);term.send(b'\x05');term.send('永久模板标记');term.send(b'\x13')
            term.expect('查看模板与变量说明')
            stored = json.loads(subprocess.check_output(base+['prompt','show']))
            assert stored['compact_generation']['system'].startswith('永久系统标记：')
            assert stored['compact_generation']['user_template'].endswith('永久模板标记')
            for field in ('system','user_template','consistency','memory','generation_preset'):
                assert stored[field] == templates_before[field]
            # The per-preset guidance toggle saves without touching other fields.
            term.choose(5)
            toggled = json.loads(subprocess.check_output(base+['prompt','show']))
            assert toggled['compact_material_guidance'] != stored['compact_material_guidance']
            # Reset restores this preset only.
            term.choose(4);term.choose(2);term.expect('编辑任务提示词模板')
            reset = json.loads(subprocess.check_output(base+['prompt','show']))
            assert reset['compact_generation'] == templates_before['compact_generation']
            for field in ('system','user_template','consistency','memory','generation_preset'):
                assert reset[field] == stored[field]
            term.home();term.choose(9);assert term.process.wait(timeout=3) == 0
        finally: term.close()
        # Narrow terminal: navigation hints and dirty confirmation remain available.
        term = Terminal(base+['tui'], width=24)
        try:
            term.expect('章节工作台')
            term.choose(2);term.choose(1);term.choose(2);term.send('窄屏稿');term.home()
            term.expect('回主页前保存？');term.send('d');term.expect('Esc 已在主页')
            term.choose(9);assert term.process.wait(timeout=3) == 0
        finally: term.close()
        # Plain mode uses the same hierarchy through 0 / H and explicit menu entries.
        term = Terminal(base+['--plain'], width=80)
        try:
            term.expect('章节工作台')
            term.choose(2);term.choose(1);term.choose(0);term.send('h'+ENTER.decode());term.choose(9)
            assert term.process.wait(timeout=3) == 0
            output=term.raw.decode('utf-8', errors='replace').replace('\r\n','\n')
            assert output.count('主页 › 章节工作台\n') >= 2
        finally: term.close()
    print('TUI navigation passed: parent/back buttons, Home, editor protection, conflicts, search, drafts, 24-column layout.')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=Path('target/debug/novel'))
    check(parser.parse_args().binary.resolve())
