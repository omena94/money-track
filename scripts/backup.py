"""Consistent PostgreSQL archives protected by Windows DPAPI, no plaintext dump files."""
import argparse
from datetime import datetime, timezone, timedelta
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import uuid
from urllib.parse import urlparse, unquote
import zipfile
from windows_dpapi import crypt

ROOT = Path(__file__).resolve().parents[1]

PG = Path(r'C:\Program Files\PostgreSQL\18\bin')
BACKUPS = ROOT / '.local/backups'
MAGIC = b'MTBK1-DPAPI\n'

def settings():
    data = (ROOT / '.local/secrets.dpapi').read_text('ascii').strip()
    return json.loads(crypt(bytes.fromhex(data), False).decode('utf-16-le'))

def pg_env(admin=False):
    env = os.environ.copy()
    for key in list(env):
        if any(part in key for part in ('DATABASE_URL', 'CLIENT_SECRET', 'ENCRYPTION_KEY', 'API_TOKEN')):
            env.pop(key, None)
    if admin:
        if not env.get('PGPASSWORD'):
            raise RuntimeError('PostgreSQL admin password is required through the process environment')
        env.setdefault('PGUSER', 'postgres')
        env.setdefault('PGHOST', 'localhost')
        env.setdefault('PGPORT', '5432')
    else:
        url = urlparse(settings()['BACKUP_DATABASE_URL'])
        if url.hostname != '127.0.0.1' or url.path != '/money_track' or url.username != 'money_backup':
            raise ValueError('Unexpected backup database')
        env.update(PGUSER=url.username, PGPASSWORD=unquote(url.password), PGHOST=url.hostname, PGPORT=str(url.port))
    env.update(PGCLIENTENCODING='UTF8', PGCONNECT_TIMEOUT='10')
    return env

def pg(tool, args, env, data=None):
    result = subprocess.run([str(PG / (tool + '.exe'))] + args, input=data, capture_output=True, env=env, timeout=300)
    if result.returncode:
        raise RuntimeError(f'{tool} failed with exit code {result.returncode}; database data was not logged')
    return result.stdout

def encrypt(payload):
    return MAGIC + crypt(payload, True)

def decrypt(archive):
    data = archive.read_bytes()
    if not data.startswith(MAGIC):
        raise ValueError('Unknown backup version')
    plain = crypt(data[len(MAGIC):], False)
    with zipfile.ZipFile(io.BytesIO(plain)) as z:
        dump = z.read('database.dump')
        metadata = json.loads(z.read('manifest.json'))
        protected = z.read('runtime.dpapi')
    if hashlib.sha256(dump).hexdigest() != metadata['dump_sha256']:
        raise ValueError('Archive checksum mismatch')
    return dump, protected

def latest():
    files = sorted(BACKUPS.glob('money-*.mtbackup'))
    if not files:
        raise ValueError('No backup available')
    return files[-1]

def create():
    BACKUPS.mkdir(parents=True, exist_ok=True)
    dump = pg('pg_dump', ['--format=custom', '--no-owner', '--dbname=money_track'], pg_env())
    if not dump.startswith(b'PGDMP'):
        raise ValueError('Invalid PostgreSQL archive')
    created = datetime.now(timezone.utc)
    metadata = dict(version=1, created_at=created.isoformat(), dump_sha256=hashlib.sha256(dump).hexdigest())
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, 'w') as z:
        z.writestr('database.dump', dump)
        z.writestr('manifest.json', json.dumps(metadata))
        z.writestr('runtime.dpapi', (ROOT / '.local/secrets.dpapi').read_bytes())
    output = BACKUPS / ('money-' + created.strftime('%Y%m%dT%H%M%S') + '-' + uuid.uuid4().hex[:6] + '.mtbackup')
    encrypted = encrypt(stream.getvalue())
    temporary = output.with_suffix('.partial')
    temporary.write_bytes(encrypted)
    # Authenticate and list the dump before promoting the file to a retained backup.
    decoded, _ = decrypt(temporary)
    pg('pg_restore', ['--list'], pg_env(), decoded)
    temporary.replace(output)
    cutoff = (created - timedelta(days=30)).timestamp()
    for old in BACKUPS.glob('money-*.mtbackup'):
        if old.resolve().parent == BACKUPS.resolve() and old.stat().st_mtime < cutoff:
            old.unlink()
    print('Encrypted backup verified: ' + str(output))
    return output

def restore(archive, database, disposable):
    pattern = r'money_restore_verify_[a-f0-9]{16}' if disposable else r'money_recovered_[a-z0-9_]{1,40}'
    if not re.fullmatch(pattern, database):
        raise ValueError('Restore target must be a new recovery database')
    dump, protected = decrypt(archive)
    env = pg_env(admin=True)
    # createdb fails if the name exists; no live database is overwritten.
    pg('createdb', ['--template=template0', database], env)
    try:
        pg('pg_restore', ['--exit-on-error', '--no-owner', '--dbname=' + database], env, dump)
        validation = pg('psql', ['-X', '-w', '-t', '-A', '-d', database, '-v', 'ON_ERROR_STOP=1', '-c', "SELECT count(*) FROM pg_tables WHERE schemaname='public'; SELECT count(*) FROM money_migrations; SELECT count(*) FROM pg_tables WHERE schemaname='public' AND rowsecurity;"], env).decode('utf-8').split()
        if len(validation) != 3 or int(validation[0]) < 14 or int(validation[1]) < 5 or int(validation[2]) < 9:
            raise ValueError('Restored schema validation failed')
        if not disposable:
            target = ROOT / '.local/recovered-secrets.dpapi'
            if target.exists():
                target = target.with_name('recovered-secrets-' + uuid.uuid4().hex[:8] + '.dpapi')
            target.write_bytes(protected)
        print('Restore verified: ' + database + ' (tables, migrations and RLS present)')
    finally:
        if disposable:
            # Exact generated prefix checked above; only this tool-created test database is dropped.
            pg('dropdb', [database], env)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['create', 'verify', 'restore-check', 'restore-new'])
    parser.add_argument('--archive', type=Path)
    parser.add_argument('--database')
    args = parser.parse_args()
    if args.action == 'create':
        create()
        return
    archive = args.archive or latest()
    if args.action == 'verify':
        dump, _ = decrypt(archive)
        pg('pg_restore', ['--list'], pg_env(), dump)
        print('Windows DPAPI integrity, checksum and PostgreSQL archive verified.')
    elif args.action == 'restore-check':
        restore(archive, 'money_restore_verify_' + uuid.uuid4().hex[:16], True)
    else:
        if not args.database:
            raise ValueError('--database is required')
        restore(archive, args.database, False)

if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print('Backup operation failed: ' + type(exc).__name__ + '; no secret or financial data logged.')
        raise SystemExit(1)
