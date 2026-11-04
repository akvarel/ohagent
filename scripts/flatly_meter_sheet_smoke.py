#!/usr/bin/env python3
"""Independent Flatly multi-row acceptance against disposable migrated databases.

Default providers are explicitly synthetic HTTP fixtures, not OCR-quality evidence.
--live uses operator-supplied OHAGENT_DOCUMENT_PROFILES and existing credential envs.
--source accepts an explicitly approved file, never copies it into the repository.
"""
import argparse
import base64
from contextlib import ExitStack
from copy import deepcopy
from decimal import Decimal
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

from accounting_document_smoke import port, wait_ready


FIELDS = ('invoice_number', 'issue_date', 'currency', 'total_amount', 'supplier_name',
          'supplier_registration_number', 'meter_number', 'reading_value', 'reading_date',
          'property_address', 'unit_number')


def artificial_rows():
    rows = []
    for unit in range(1, 8):
        for period, value in [('30.09', f'{unit * 10}.001'), ('31.10', f'{unit * 10 + 1}.101')]:
            rows.append({'row_id': f'row-{len(rows) + 1}', 'unit_number': str(unit),
                         'meter_number': None, 'owner_name': f'Artificial owner {unit}',
                         'reading_value': value, 'reading_date': None, 'reading_date_text': period,
                         'evidence': [{'field': 'reading_value', 'page': 1, 'quote': value}],
                         'warnings': ['Artificial HTTP fixture. Year must be entered by the reviewer.']})
    return rows


class MeterSheetFixture:
    def __init__(self):
        self.servers = []
        self.threads = []
        self.calls = []

    def __enter__(self):
        for index in range(3):
            fixture = self

            class Handler(BaseHTTPRequestHandler):
                slot = index

                def log_message(self, *_):
                    pass

                def do_POST(self):
                    if self.path != '/v1/chat/completions':
                        self.send_error(404)
                        return
                    count = int(self.headers.get('Content-Length', '0'))
                    if not 0 < count <= 20 * 1024 * 1024:
                        self.send_error(413)
                        return
                    request = json.loads(self.rfile.read(count))
                    fixture.calls.append({'slot': self.slot, 'model': request.get('model')})
                    rows = artificial_rows()
                    # Retain a visible third-model disagreement, never a merged value.
                    if self.slot == 2:
                        rows[-1]['reading_value'] = '71.102'
                    fields = dict.fromkeys(FIELDS)
                    fields['property_address'] = '1 Test Street, Riga'
                    extraction = {'document_type': 'meter_reading', 'fields': fields,
                                  'meter_readings': rows, 'evidence': [],
                                  'warnings': ['Artificial provider, not real OCR.'], 'model': 'untrusted-label'}
                    body = json.dumps({'choices': [{'finish_reason': 'stop', 'message': {
                        'content': json.dumps(extraction)}}]}).encode()
                    self.send_response(200)
                    self.send_header('Content-Type', 'application/json')
                    self.send_header('Content-Length', str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

            server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            self.servers.append(server)
            self.threads.append(thread)
        return self

    def environment(self, directory):
        env = {'OHAGENT_DOCUMENT_MODELS_FILE': str(directory / 'models.json')}
        profiles = []
        for i, server in enumerate(self.servers):
            name = f'ARTIFICIAL_METER_PROFILE_KEY_{i}'
            env[name] = secrets.token_hex(16)
            profiles.append({'id': f'fixture-{i}', 'label': f'Artificial model {i}',
                             'base_url': f'http://127.0.0.1:{server.server_port}/v1',
                             'api_key_env': name, 'model': f'artificial-meter-model-{i}',
                             'json_mode': 'json_object', 'pdf_mode': 'native', 'enabled': True})
        env['OHAGENT_DOCUMENT_PROFILES'] = json.dumps(profiles)
        return env

    def __exit__(self, *_):
        for server in self.servers:
            server.shutdown()
            server.server_close()
        for thread in self.threads:
            thread.join(timeout=5)


def generated_image(marker='independent-api'):
    from PIL import Image, ImageDraw, ImageFont
    image = Image.new('RGB', (1200, 1300), 'white')
    draw = ImageDraw.Draw(image)
    font = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf', 24)
    lines = ['ARTIFICIAL WATER METER SHEET', '1 Test Street, Riga', marker,
             'Unit / previous 30.09 / current 31.10 / owner. YEAR NOT STATED.']
    for unit in range(1, 8):
        lines.append(f'{unit} / {unit * 10}.001 / {unit * 10 + 1}.101 / Artificial owner {unit}')
    for i, line in enumerate(lines):
        draw.text((35, 35 + 65 * i), line, font=font, fill='black')
    stream = io.BytesIO()
    image.save(stream, 'PNG')
    return stream.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--backend-jar', required=True)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--live', action='store_true')
    parser.add_argument('--analyze-only', action='store_true')
    parser.add_argument('--review-year', type=int, help='Explicit human choice. Never inferred from the document.')
    parser.add_argument('--browser-seconds', type=int, default=0, choices=range(0, 901))
    parser.add_argument('--private-results', type=Path, help='Optional private local result directory. Never use a Git repository.')
    args = parser.parse_args()
    if args.live and not os.environ.get('OHAGENT_DOCUMENT_PROFILES'):
        parser.error('--live requires existing operator-owned OHAGENT_DOCUMENT_PROFILES')
    if args.live and not args.analyze_only and args.review_year is None:
        parser.error('Applying live rows requires an explicit --review-year')
    if args.source and not args.live:
        parser.error('--source requires --live, not synthetic OCR fixtures')
    if args.review_year is not None and not 1900 <= args.review_year <= 9999:
        parser.error('Invalid explicit year')
    # A fixed artificial test decision, not a date guessed from the uploaded image.
    review_year = args.review_year if args.review_year is not None else 2025

    processes = []

    def stop(*_):
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    with ExitStack() as cleanup:
        temporary = Path(cleanup.enter_context(tempfile.TemporaryDirectory(prefix='flatly-meter-sheet-')))
        fixture = None if args.live else cleanup.enter_context(MeterSheetFixture())
        log = cleanup.enter_context((temporary / 'startup.log').open('w+'))
        core_port, app_port = port(), port()
        core_env = dict(os.environ)
        core_env.update(OHAGENT_API_KEY=secrets.token_hex(24),
                        OHAGENT_DOCUMENT_BIND=f'127.0.0.1:{core_port}',
                        OHAGENT_DOCUMENT_MODELS_FILE=str(temporary / 'models.json'))
        if fixture:
            core_env.update(fixture.environment(temporary))
        profiles = json.loads(core_env['OHAGENT_DOCUMENT_PROFILES'])
        credential_names = {profile['api_key_env'] for profile in profiles}

        def terminate():
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
        cleanup.callback(terminate)
        core = subprocess.Popen([str(Path(args.binary).resolve())], env=core_env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(core)
        wait_ready(core, ('127.0.0.1', core_port))
        blocked_names = credential_names | {'SENDGRID_API_KEY', 'VAULT_TOKEN', 'GOOGLE_OAUTH_CLIENT_ID',
                                          'GOOGLE_OAUTH_CLIENT_SECRET', 'OHAGENT_DOCUMENT_ADMIN_KEY',
                                          'HOUSEHOLD_DOCUMENTS_ADMIN_KEY'}
        app_env = {name: value for name, value in os.environ.items() if name not in blocked_names}
        app_env.update(JWT_SECRET=secrets.token_hex(32),
                       DATASOURCES_DEFAULT_URL=f'jdbc:h2:file:{temporary}/database;DB_CLOSE_DELAY=-1;MODE=PostgreSQL;DATABASE_TO_UPPER=false',
                       MICRONAUT_SERVER_HOST='127.0.0.1', MICRONAUT_SERVER_PORT=str(app_port),
                       MICRONAUT_SECURITY_OAUTH2_ENABLED='false', ACCEPTANCE_SEED_ENABLED='true',
                       EMAIL_AUTOMATED_ENABLED='false', SMS_ALLOWED='false', PUSH_ALLOWED='false',
                       HOUSEHOLD_ASSIST_SLM_ENABLED='false', PAYMENT_PROVIDER='manual',
                       APP_PUBLIC_URL=f'http://127.0.0.1:{app_port}',
                       OHAGENT_DOCUMENT_URL=f'http://127.0.0.1:{core_port}/api/document-processing/extract',
                       OHAGENT_API_KEY=core_env['OHAGENT_API_KEY'], DOCUMENT_EXTRACTION_PROVIDER='ohagent')
        app = subprocess.Popen(['java', '-Xmx768m', '-jar', str(Path(args.backend_jar).resolve())],
                               cwd=temporary, env=app_env, stdout=log, stderr=log)
        processes.append(app)
        for _ in range(600):
            if app.poll() is not None:
                log.flush()
                raise RuntimeError('Isolated backend exited: ' + (temporary / 'startup.log').read_text()[-6000:])
            try:
                with socket.create_connection(('127.0.0.1', app_port), timeout=.1):
                    break
            except OSError:
                time.sleep(.1)
        else:
            raise RuntimeError('Isolated backend startup exceeded 60 seconds')
        base = f'http://127.0.0.1:{app_port}'
        bearer = None

        def request(method, path, data=None, expected=200, authenticated=True):
            headers = {'Content-Type': 'application/json'}
            if authenticated and bearer:
                headers['Authorization'] = 'Bearer ' + bearer
            req = urllib.request.Request(base + path, data=json.dumps(data).encode() if data is not None else None,
                                         headers=headers, method=method)
            try:
                with urllib.request.urlopen(req, timeout=120) as response:
                    status, raw = response.status, response.read()
            except urllib.error.HTTPError as error:
                status, raw = error.code, error.read()
            assert status == expected, (method, path, status, expected, raw[:1500])
            return json.loads(raw) if raw[:1] in (b'{', b'[') else raw

        def login(email):
            result = request('POST', '/api/auth/login', {'email': email, 'password': 'TestPass123!'}, authenticated=False)
            token = result.get('accessToken') or result.get('token') or result.get('access_token')
            assert token
            return token

        bearer = login('acceptance-manager@test.local')
        manager = bearer
        outsider = login('acceptance-nocompany@test.local')
        data = args.source.read_bytes() if args.source else generated_image()
        extension = args.source.suffix.lower() if args.source else '.png'
        mime = {'.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.png': 'image/png', '.pdf': 'application/pdf'}[extension]
        upload = {'companyName': 'Acceptance Company', 'fileName': 'approved-meter-sheet' + extension,
                  'mediaType': mime, 'contentBase64': base64.b64encode(data).decode()}
        job = request('POST', '/api/document-jobs', upload)
        path = '/api/document-jobs/' + job['id']
        assert request('GET', path + '/source') == data
        bearer = outsider
        request('GET', path, expected=403)
        request('GET', path + '/source', expected=403)
        bearer = manager
        job = request('POST', path + '/analyze')
        assert job['status'] == 'REVIEW_REQUIRED', job
        rows = job['extraction']['meterReadings']
        comparison = deepcopy(job['comparison'])
        assert len(rows) == 14, ('Expected 14 retained observations', len(rows))
        assert len({row['rowId'] for row in rows}) == 14
        assert all(row['readingDate'] is None and row['readingDateText'] for row in rows)
        assert len(comparison['results']) == 3
        if fixture:
            assert comparison['complete'] and comparison['disagreements']
            assert len(fixture.calls) == 3
        print(json.dumps({'status': 'PASS', 'check': 'actual migrated intake and comparison',
                          'observations': len(rows), 'original_retained': True,
                          'provider_kind': 'live configured providers' if args.live else 'three local HTTP fixtures',
                          'model_outcomes': [{key: result.get(key) for key in ('model', 'status', 'errorCode')}
                                             for result in comparison['results']]}), flush=True)
        if args.private_results:
            args.private_results.mkdir(parents=True, exist_ok=True, mode=0o700)
            retained = args.private_results / 'multirow-job.json'
            descriptor = os.open(retained, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(descriptor, 'w') as stream:
                json.dump(job, stream, ensure_ascii=False, indent=2)
        if args.analyze_only:
            return

        def create_meters(prefix):
            result = {}
            for unit in range(1, 8):
                apartment = request('POST', '/api/units', {'companyName': 'Acceptance Company',
                                    'householdObject': 'acceptance-object-1', 'apartmentNumber': f'{prefix}-{unit}'}, expected=201)
                meter = request('POST', '/api/utility-meters', {'objectId': 'acceptance-object-1',
                                'unitId': apartment['id'], 'type': 'cold_water',
                                'serialNumber': f'{prefix}-METER-{unit}', 'status': 'active'}, expected=201)
                result[str(unit)] = meter['id']
            return result

        def domain_rows(meter):
            return request('GET', '/api/utility-meters/readings?meterId=' + meter)['content']

        targets = create_meters('SHEET-API')
        decisions = []
        for row in rows:
            day, month = row['readingDateText'].strip('.').split('.')
            decisions.append({'rowId': row['rowId'], 'meterId': targets[row['unitNumber']],
                              'readingValue': row['readingValue'],
                              'readingDate': f'{review_year}-{int(month):02d}-{int(day):02d}', 'reviewed': True})
        approval = {'sourceHash': job['sourceHash'], 'documentType': 'meter_reading',
                    'fields': job['extraction']['fields'], 'comparisonReviewed': True, 'meterReadings': decisions}
        incomplete = deepcopy(approval)
        incomplete['meterReadings'].pop()
        request('POST', path + '/approve-apply', incomplete, expected=400)
        unreviewed = deepcopy(approval)
        unreviewed['meterReadings'][0]['reviewed'] = False
        request('POST', path + '/approve-apply', unreviewed, expected=400)
        no_year = deepcopy(approval)
        no_year['meterReadings'][0]['readingDate'] = None
        request('POST', path + '/approve-apply', no_year, expected=400)
        scalar = {key: value for key, value in approval.items() if key != 'meterReadings'}
        scalar.update(meterId=next(iter(targets.values())))
        request('POST', path + '/approve-apply', scalar, expected=400)
        # Seed a real conflict on the final meter, forcing rollback after earlier writes.
        last = decisions[-1]
        conflict = request('POST', '/api/utility-meters/readings', {'meterId': last['meterId'],
                           'readingValue': '9999', 'readingDate': last['readingDate'], 'source': 'manual'}, expected=201)
        request('POST', path + '/approve-apply', approval, expected=409)
        assert request('GET', path)['status'] == 'REVIEW_REQUIRED'
        for meter in targets.values():
            assert len(domain_rows(meter)) == (1 if meter == last['meterId'] else 0), 'Partial batch escaped rollback'
        # Explicit human correction to match the existing statement is a domain receipt.
        corrected = deepcopy(approval)
        corrected['meterReadings'][-1]['readingValue'] = '9999'
        bearer = outsider
        request('POST', path + '/approve-apply', corrected, expected=403)
        bearer = manager
        applied = request('POST', path + '/approve-apply', corrected)
        assert applied['status'] == 'APPLIED' and len(applied['resultIds']) == 14, applied
        assert len(set(applied['resultIds'])) == 14
        assert applied['comparison'] == comparison, 'Review changed original model evidence'
        assert applied['extraction'] == job['extraction']
        stored_decision = json.loads(applied['reviewedJson'])
        assert stored_decision['meterReadings'][-1]['readingValue'] == '9999'
        for row in corrected['meterReadings']:
            stored = domain_rows(row['meterId'])
            assert len(stored) == 2
            assert any(item['readingDate'] == row['readingDate']
                       and Decimal(str(item['readingValue'])) == Decimal(row['readingValue'])
                       for item in stored), 'Canonical meter history differs from reviewed row'
        replayed = request('POST', path + '/approve-apply', corrected)
        assert replayed['resultIds'] == applied['resultIds']
        assert request('POST', '/api/document-jobs', upload)['id'] == job['id']
        assert request('GET', path + '/source') == data
        print(json.dumps({'status': 'PASS', 'check': '14-row atomic application and canonical readback',
                          'result_count': 14, 'late_conflict_rolled_back': True,
                          'explicit_correction_retained': True, 'original_comparison_unchanged': True,
                          'duplicate_effects': False, 'source_scope_checked': True}), flush=True)
        if args.browser_seconds:
            browser_data = generated_image('independent-browser')
            browser_upload = dict(upload, fileName='artificial-browser-meter-sheet.png', mediaType='image/png',
                                  contentBase64=base64.b64encode(browser_data).decode())
            browser_job = request('POST', '/api/document-jobs', browser_upload)
            browser_job = request('POST', '/api/document-jobs/' + browser_job['id'] + '/analyze')
            browser_targets = create_meters('SHEET-BROWSER')
            print(json.dumps({'status': 'BROWSER_READY', 'backend_url': base,
                              'job_id': browser_job['id'], 'meters_by_unit': browser_targets,
                              'lifetime_seconds': args.browser_seconds}), flush=True)
            time.sleep(args.browser_seconds)


if __name__ == '__main__':
    main()
