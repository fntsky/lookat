#!/usr/bin/env python3
"""使用真实 Formicary 二进制与隔离测试数据库，验证 API 密钥代理链路。"""
import argparse
import http.cookies
import http.server
import json
import os
from pathlib import Path
import re
import shlex
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
import uuid

ROOT = Path(__file__).resolve().parents[1]
FORMICARY = Path(os.environ.get('FORMICARY_ROOT', ROOT.parent / 'formicary')).resolve()
parser = argparse.ArgumentParser()
parser.add_argument('--browser', action='store_true', help='完成测试后提供一次性浏览器入口，输入换行清理退出')
args = parser.parse_args()
subprocess.run(['cargo','build','--bin','lookat-server'],cwd=ROOT,check=True)
binary = FORMICARY / 'target/release/Formicary'
if not binary.is_file(): raise SystemExit('请先在 Formicary 仓库构建 release 二进制')
(FORMICARY / 'tmp').mkdir(exist_ok=True)

def free_port():
    with socket.socket() as server:
        server.bind(('127.0.0.1',0)); return server.getsockname()[1]

with tempfile.TemporaryDirectory(prefix='lookat-e2e-', dir=FORMICARY / 'tmp') as temporary:
    root = Path(temporary)
    chamber = root / 'chamber/lookat'
    chamber.mkdir(parents=True)
    (root / 'ui').symlink_to(FORMICARY / 'ui', target_is_directory=True)
    port = free_port()
    command = 'exec ' + shlex.quote(str(ROOT / 'target/debug/lookat-server'))
    (chamber / 'chamber.toml').write_text('[chamber]\nname="lookat"\nschema_version=1\n[proxyhttp]\nworking_directory="."\nscript='+json.dumps(command)+'\nupstream="http://127.0.0.1:'+str(port)+'"\nroute="/lookat"\n')
    env = dict(os.environ, FORMICARY_HOST='127.0.0.1', FORMICARY_PORT='0',
               FORMICARY_DATABASE_PATH=str(root / 'auth.sqlite3'), FORMICARY_CHAMBERS_DIR=str(root / 'chamber'),
               LOOKAT_PORT=str(port), LOOKAT_DATA_PATH=str(root / 'usage.sqlite3'),
               LOOKAT_FRONTEND_DIR=str(ROOT / 'frontend/dist'))
    with (root / 'server.log').open('w+') as log:
        os.chmod(root / 'server.log',0o600)
        process = subprocess.Popen([str(binary)],cwd=root,env=env,stdout=log,stderr=log)
        try:
            base = None; password = None
            for _ in range(100):
                contents = (root / 'server.log').read_text()
                address = re.search(r'HTTP server listening on (http://127\.0\.0\.1:\d+)',contents)
                secret = re.search(r'密码: (\S+)',contents)
                if address and secret:
                    base = address.group(1); password = secret.group(1); break
                if process.poll() is not None: raise AssertionError('Formicary 启动失败，未输出日志以避免泄露临时凭据')
                time.sleep(.1)
            assert base and password,'等待 Formicary 启动超时'
            cookie = None
            def request(path,method='GET',body=None,headers=None,session=False,expected=200):
                actual = {'Content-Type':'application/json',**(headers or {})}
                if session: actual['Cookie']=cookie
                req=urllib.request.Request(base+path,method=method,headers=actual,
                    data=None if body is None else json.dumps(body).encode())
                try: response=urllib.request.urlopen(req,timeout=12)
                except urllib.error.HTTPError as error: response=error
                with response:
                    payload=response.read(); assert response.status==expected,(path,response.status)
                    data=json.loads(payload) if 'application/json' in response.headers.get('Content-Type','') else payload
                    return data,response.headers
            result,headers=request('/api/login','POST',{'username':'admin','password':password})
            cookies=http.cookies.SimpleCookie()
            for value in headers.get_all('Set-Cookie',[]): cookies.load(value)
            cookie='formicary_session='+cookies['formicary_session'].value
            created,_=request('/api/keys','POST',{'name':'lookat-integration','current_password':password},session=True,expected=201)
            token=created['token']; key_id=created['key']['id']; authorized={'Authorization':'Bearer '+token}
            for _ in range(50):
                try:
                    request('/lookat/healthz'); break
                except (AssertionError,urllib.error.URLError): time.sleep(.1)
            else: raise AssertionError('Lookat upstream 启动超时')
            request('/lookat/api/v1/devices')
            request('/lookat/api/v1/batches','POST',{},session=True,expected=403)
            spoof={'X-Formicary-Auth':'api_key','X-Formicary-User':'admin',
                   'X-Formicary-Principal':'api-key:0123456789abcdef01234567','X-Formicary-Key-Id':'0123456789abcdef01234567'}
            request('/lookat/api/v1/batches','POST',{},headers=spoof,expected=401)
            request('/lookat/api/v1/batches','POST',{},headers={'X-API-Key':'invalid'},expected=401)
            batch=json.loads((ROOT / 'docs/examples/active-batch.json').read_text())
            now=datetime.now(timezone.utc).replace(microsecond=0)
            stamp=lambda t:t.isoformat(timespec='milliseconds').replace('+00:00','Z')
            batch.update(batch_id=str(uuid.uuid4()),stream_id=str(uuid.uuid4()),sequence=0,sent_at=stamp(now))
            batch['device'].update(id=str(uuid.uuid4()),name='Integration Linux')
            batch['intervals'][0].update(id=str(uuid.uuid4()),started_at=stamp(now-timedelta(seconds=15)),ended_at=stamp(now))
            batch['snapshot'].update(observed_at=stamp(now),since=stamp(now-timedelta(seconds=872)))
            receipt,_=request('/lookat/api/v1/batches','POST',batch,headers=authorized)
            assert receipt['accepted_intervals']==1
            duplicate,_=request('/lookat/api/v1/batches','POST',batch,headers={'X-API-Key':token})
            assert duplicate['status']=='duplicate'
            query='/lookat/api/v1/overview?device_id='+batch['device']['id']
            view,_=request(query)
            assert view['total_ms']==15000 and view['current']['name']=='Visual Studio Code'
            request('/api/keys/'+str(key_id),'DELETE',session=True)
            request('/lookat/api/v1/batches','POST',batch,headers=authorized,expected=401)
            print('Formicary 全链路通过：公开查询、密钥上报、伪造头拒绝、无效/撤销密钥拒绝、重复去重。',flush=True)
            if args.browser:
                print('浏览器验证入口：'+base+'/lookat/',flush=True)
                print('公开浏览验证中；输入换行结束隔离测试环境。',flush=True)
                input()
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                try: process.wait(timeout=15)
                except subprocess.TimeoutExpired: process.kill();process.wait(timeout=5)
