"""Verify authenticated event streaming through each actual HTTP entry point."""
import json
import os
from pathlib import Path
import subprocess
import time
import requests

ROOT=Path(__file__).resolve().parents[2]
FIXTURE=ROOT/'target/debug/examples/ui_fixture.exe'
identity=json.loads(subprocess.run([str(FIXTURE)],capture_output=True,check=True,encoding='utf-8').stdout)
try:
    for origin in dict.fromkeys(['http://127.0.0.1:3001','http://localhost:3000'] + ([os.environ['APP_ORIGIN'].rstrip('/')] if os.environ.get('APP_ORIGIN') else [])):
        start=time.monotonic()
        try:
            with requests.get(origin+'/api/v1/sync/events',cookies={'money_session':identity['session']},headers={'Accept':'text/event-stream','Accept-Encoding':'gzip'},stream=True,timeout=(10,5)) as response:
                response.raise_for_status()
                for line in response.iter_lines(chunk_size=1):
                    if line.startswith(b'data:'):
                        assert json.loads(line[5:])==[]
                        print(f'{origin}: first authenticated SSE event in {time.monotonic()-start:.2f}s, encoding={response.headers.get("Content-Encoding","identity")}')
                        break
                else:raise AssertionError('Event stream closed without a jobs event')
        except Exception as error:
            print(f'{origin}: SSE check failed ({type(error).__name__})')
            raise
finally:
    subprocess.run([str(FIXTURE),identity['id']],capture_output=True,check=True)
