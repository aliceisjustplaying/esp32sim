#!/usr/bin/env python3
"""Compare matched, uninstrumented capture-battery runs and their console milestones."""
import argparse
import hashlib
import json
import pathlib
import statistics

# Intervals include all work between the preceding marker and this marker, including
# setup and console delivery. They are not isolated function or guest-device timings.
MILESTONES = (
    ('boot_and_native_kernels', 'TINYDRAW_GATE1_NATIVE_KERNELS'),
    ('cold_render_and_initial_pan', 'TINYDRAW_GATE1_RING_LOCAL'),
    ('pan_sequences', 'TINYDRAW_GATE1_PAN_BOUNDARY'),
    ('cache_tour', 'TINYDRAW_GATE1_CACHE_TOUR'),
    ('mixed_drawing', 'TINYDRAW_GATE1_MIXED_DRAW_SUMMARY'),
    ('hairlines', 'TINYDRAW_GATE1_HAIRLINE'),
    ('export', 'TINYDRAW_GATE1_EXPORT'),
    ('history', 'TINYDRAW_GATE1_HISTORY_SUMMARY'),
    ('settling', 'TINYDRAW_GATE1_AUTOMATED_DONE'),
)


def read_run(directory):
    directory = pathlib.Path(directory)
    capture = json.loads((directory / 'result.json').read_text())
    result, version = capture['result'], capture['version']
    if not result['passed'] or result['status'] != 'completed':
        raise ValueError(f'{directory}: firmware did not complete successfully')
    if 'HeadlessChrome/' not in version['User-Agent']:
        raise ValueError(f'{directory}: expected a headless Chrome timing capture')
    events = json.loads((directory / 'events.json').read_text())
    if any(event.get('type') in ('emu', 'log')
           and any(marker in event.get('line', '') for marker in ('jit-profile', '[wasm-profile]'))
           for event in events):
        raise ValueError(f'{directory}: profiling captures cannot establish production speed')
    serial, pending, times = '', '', {}
    markers = {marker for _, marker in MILESTONES}
    for event in events:
        if event.get('type') != 'serial':
            continue
        serial += event['data']
        pending += event['data']
        lines = pending.split('\n')
        pending = lines.pop()
        for line in lines:
            marker = line.split(' ', 1)[0].strip()
            if marker in markers:
                times[marker] = event['wallMs'] / 1000
    intervals, previous = {}, 0
    for name, marker in MILESTONES:
        end = times.get(marker)
        if end is None or end < previous:
            raise ValueError(f'{directory}: missing or out-of-order milestone {marker}')
        intervals[name] = end - previous
        previous = end
    return {
        'directory': str(directory),
        'wallSeconds': result['wallSeconds'],
        'intervalSeconds': intervals,
        'instructions': result['instructions'],
        'jitInstructions': result['jit']['instructions'],
        'consoleSha256': hashlib.sha256(serial.encode()).hexdigest(),
        'verdict': result['verdict'],
        'browser': version['Browser'],
        'v8': version['V8-Version'],
    }


def comparison(baseline, candidate):
    runs = baseline + candidate
    for field in ('instructions', 'consoleSha256', 'verdict', 'browser', 'v8'):
        if len({run[field] for run in runs}) != 1:
            raise ValueError(f'Unmatched {field}; inspect the runs before comparing performance')

    def metric(get):
        before, after = [get(run) for run in baseline], [get(run) for run in candidate]
        a, b = statistics.median(before), statistics.median(after)
        return {'baseline': before, 'candidate': after, 'baselineMedian': a,
                'candidateMedian': b, 'lessWallTimePercent': (1 - b / a) * 100 if a else None}

    return {
        'baseline': baseline,
        'candidate': candidate,
        'total': metric(lambda run: run['wallSeconds']),
        'intervals': {name: metric(lambda run: run['intervalSeconds'][name]) for name, _ in MILESTONES},
        'scope': 'Host wall time between firmware console milestones; includes setup and console delivery. '
                 'Not isolated function timings, input latency or silicon cycle accuracy. '
                 'Matching firmware/build inputs and absence of profiling must also be verified from capture provenance.',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', nargs='+', required=True, help='Capture directories, one per run')
    parser.add_argument('--candidate', nargs='+', required=True, help='Capture directories, one per run')
    args = parser.parse_args()
    try:
        result = comparison([read_run(p) for p in args.baseline], [read_run(p) for p in args.candidate])
    except (ValueError, KeyError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
