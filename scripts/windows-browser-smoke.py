"""Open a local fixture and capture the actual WebView2 panel through Desktop's stdio MCP."""

import base64
import functools
import http.server
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import threading
import time


def smoke(exe, root):
    project = root / 'project'
    project.mkdir()
    (project / 'index.html').write_text('''<!doctype html><title>Panel fixture</title>
        <form onsubmit="event.preventDefault(); window.submitted = true">
        <input id="entry" onkeydown="if(event.key==='x') event.preventDefault()">
        <select id="choice"><option>First</option><option disabled>Skip</option><option>Last</option></select>
        <button>Smoke button</button></form>''')
    env = {**os.environ, 'APPDATA': str(root / 'roaming'), 'LOCALAPPDATA': str(root / 'local'),
           'WEBVIEW2_USER_DATA_FOLDER': str(root / 'webview')}

    def rpc(method, **params):
        request = {'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}
        reply = subprocess.run([exe, '--browser-mcp'], cwd=project, env=env, text=True,
                               input=json.dumps(request) + '\n', stdout=subprocess.PIPE,
                               timeout=45, check=True)
        return json.loads(reply.stdout)['result']

    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(project))
    with http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler) as server:
        threading.Thread(target=server.serve_forever, daemon=True).start()
        app = subprocess.Popen([exe, '--preview=empty', project], cwd=project, env=env)
        try:
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                if app.poll() is not None:
                    raise RuntimeError(f'Desktop exited during startup: {app.returncode}')
                if any(t['name'] == 'browser_open' for t in rpc('tools/list')['tools']):
                    # The bridge can be ready before the frontend's open-event listener.
                    page = rpc('tools/call', name='browser_open', arguments={
                        'url': f'http://127.0.0.1:{server.server_port}/'})
                    if not page['isError'] and 'Smoke button' in page['content'][0]['text']:
                        break
                time.sleep(0.5)
            else:
                raise RuntimeError('Browser panel did not load the local fixture')
            def call(name, **args):
                result = rpc('tools/call', name=name, arguments=args)
                assert not result['isError'], result['content'][0]['text']
                return result['content'][0]['text']

            call('browser_click', selector='#entry')
            for key in ('a', 'b', 'ArrowLeft', '!', 'Delete', 'Backspace', 'x', 'Enter',
                        'Tab', 'ArrowDown', 'Shift+Tab'):
                call('browser_press', key=key)
            state = call('browser_eval', script='''return [document.querySelector('#entry').value,
                document.querySelector('#choice').value, document.activeElement.id, window.submitted]''')
            assert json.loads(state) == ['a', 'Last', 'entry', True], state
            assert 'no native default' in call('browser_press', key='Escape')
            assert rpc('tools/call', name='browser_press', arguments={'key': 'Ctrl+L'})['isError']
            reply = rpc('tools/call', name='browser_screenshot', arguments={})
            assert not reply['isError'], reply['content'][0]['text']
            image, = [part for part in reply['content'] if part['type'] == 'image']
            assert image['mimeType'] == 'image/png'
            png = base64.b64decode(image['data'], validate=True)
            assert len(png) <= 5 * 1024 * 1024
            assert png[:16] == b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR'
            assert min(struct.unpack('>II', png[16:24])) > 1
            assert png.endswith(b'IEND\xaeB`\x82')
        finally:
            app.kill()
            app.wait(timeout=10)
            server.shutdown()


if __name__ == '__main__':
    with tempfile.TemporaryDirectory() as tmp:
        smoke(Path(sys.argv[1]).resolve(strict=True), Path(tmp))
