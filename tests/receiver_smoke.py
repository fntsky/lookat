#!/usr/bin/env python3
"""真实 HTTP 接收/持久化测试；可信头仅模拟本机 Formicary 转发。"""
import json, os, re, subprocess, tempfile, urllib.error, urllib.request, uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
subprocess.run(['cargo', 'build', '--bin', 'lookat-server'], cwd=ROOT, check=True)

def packet():
    batch = json.loads((ROOT / 'docs/examples/active-batch.json').read_text())
    now = datetime.now(timezone.utc).replace(microsecond=0)
    stamp = lambda t: t.isoformat(timespec='milliseconds').replace('+00:00', 'Z')
    batch.update(batch_id=str(uuid.uuid4()), stream_id=str(uuid.uuid4()), sequence=0, sent_at=stamp(now))
    batch['device']['id'] = str(uuid.uuid4())
    batch['intervals'][0].update(id=str(uuid.uuid4()), started_at=stamp(now-timedelta(seconds=15)), ended_at=stamp(now))
    batch['snapshot'].update(observed_at=stamp(now), since=stamp(now-timedelta(seconds=872)))
    return batch

with tempfile.TemporaryDirectory(prefix='lookat-receiver-') as temporary:
    env = dict(os.environ, LOOKAT_PORT='0', LOOKAT_FRONTEND_DIR=str(ROOT / 'frontend/dist'),
               LOOKAT_DATA_PATH=str(Path(temporary) / 'lookat.sqlite3'),
               DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent/lookat-test-bus')
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP']: env.pop(key, None)
    def start():
        process = subprocess.Popen([str(ROOT / 'target/debug/lookat-server')], cwd=temporary,env=env,stdout=subprocess.PIPE,text=True)
        line=process.stdout.readline(); address=re.search(r'http://127\.0\.0\.1:\d+',line)
        if not address:
            process.terminate(); process.wait(timeout=10)
            raise AssertionError('接收服务启动失败')
        return process,address.group()
    process, base = start()
    def request(path, method='GET', body=None, auth=None, expected=200):
        headers={'Content-Type':'application/json'}
        if auth:
            headers.update({'X-Formicary-Auth':auth,'X-Formicary-User':'test-owner',
                            'X-Formicary-Principal':'api-key:0123456789abcdef01234567' if auth=='api_key' else 'test-owner'})
            if auth=='api_key': headers['X-Formicary-Key-Id']='0123456789abcdef01234567'
        req=urllib.request.Request(base+path,method=method,headers=headers,data=None if body is None else json.dumps(body).encode())
        try: response=urllib.request.urlopen(req,timeout=8)
        except urllib.error.HTTPError as error: response=error
        with response:
            data=response.read(); assert response.status==expected,(path,response.status,data)
            return json.loads(data) if 'application/json' in response.headers.get('Content-Type','') else data
    try:
        assert b'<div id="app">' in request('/')
        assert request('/healthz')==b'ok'
        request('/api/v1/devices',expected=401)
        request('/api/v1/batches','POST',{},auth='session',expected=403)
        batch=packet()
        result=request('/api/v1/batches','POST',batch,auth='api_key');assert result['accepted_intervals']==1
        assert request('/api/v1/batches','POST',batch,auth='api_key')['status']=='duplicate'
        query='/api/v1/overview?device_id='+batch['device']['id']
        assert request(query,auth='session')['total_ms']==15000
        process.terminate();process.wait(timeout=10);process,base=start()
        assert request(query,auth='session')['total_ms']==15000
        for name in ['ingestion-v1.md','ingestion-v1.schema.json','active-batch.json','locked-heartbeat.json']:
            request('/protocol/'+name)
        print('接收端 HTTP 检查通过：鉴权边界、幂等、查询、重启恢复、协议下载，无桌面依赖。')
    finally:
        process.terminate();process.wait(timeout=10)
