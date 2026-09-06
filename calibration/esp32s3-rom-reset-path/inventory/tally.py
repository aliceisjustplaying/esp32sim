#!/usr/bin/env python3
"""Tally an esp32sim --trace log of the ROM reset path. Instruction counts only; no timing.
Usage: tally.py <trace.log or .gz> <rom.dis>
Limitation: the trace prints a0..a3 only, so load/store effective addresses are not
recoverable for arbitrary base registers; no access-target classification is made."""
import re,sys,collections,bisect,gzip
trace,romdis=sys.argv[1],sys.argv[2]
opener=gzip.open if trace.endswith('.gz') else open
syms=[]
for l in open(romdis):
    m=re.match(r'^([0-9a-f]{8}) <(\S+)>:',l)
    if m: syms.append((int(m.group(1),16),m.group(2)))
syms.sort(); addrs=[a for a,_ in syms]
def sym(pc):
    i=bisect.bisect_right(addrs,pc)-1
    return syms[i][1] if i>=0 else '?'
def region(pc):
    if 0x40000000<=pc<0x40060000: return 'ROM'
    if 0x40370000<=pc<0x403E0000: return 'IRAM'
    if 0x42000000<=pc<0x44000000: return 'flash(icache)'
    return 'other'
def klass(mn):
    if mn.startswith('call'): return 'call'
    if mn.startswith('ret'): return 'return'
    if mn in ('j','jx'): return 'jump'
    if mn.startswith('b') and not mn.startswith('break'): return 'branch'
    if mn.startswith('loop'): return 'loop-setup'
    if mn=='l32r': return 'literal-load'
    if mn.startswith(('l8','l16','l32')): return 'load'
    if mn.startswith(('s8','s16','s32')): return 'store'
    if mn.startswith(('rsr','wsr','xsr','rsil','waiti','isync','rsync','esync','dsync','memw','extw','rfe','rfi','rotw','movsp','rur','wur')): return 'special'
    if mn=='entry': return 'entry'
    return 'alu'
pat=re.compile(r'^\s*(\d+)\s+([0-9a-f]{8}):\s+(\S+)\s*(.*?)\s{2,}a0=')
rows=[]
for l in opener(trace,'rt'):
    m=pat.match(l)
    if m: rows.append((int(m.group(2),16),m.group(3),m.group(4)))
print('instructions traced:',len(rows))
print('by fetch region:',dict(collections.Counter(region(pc) for pc,_,_ in rows)))
cls=collections.Counter((region(pc),klass(mn)) for pc,mn,_ in rows)
for reg in ['ROM','IRAM','flash(icache)','other']:
    d={k:v for (r,k),v in cls.items() if r==reg}
    if d: print(f'  {reg:14s}',dict(sorted(d.items(),key=lambda x:-x[1])))
out=collections.Counter()
for i,(pc,mn,op) in enumerate(rows[:-1]):
    if region(pc)=='ROM' and (klass(mn)=='branch' or mn=='j'):
        ln=2 if mn.endswith('.n') else 3
        fall = rows[i+1][0]==pc+ln
        key = 'j' if mn=='j' else 'branch'
        out[(key, 'next PC == fallthrough' if fall else 'next PC != fallthrough')]+=1
print('ROM branch/jump next-PC metric (a j whose target is the next instruction counts as fallthrough):',dict(out))
lit=collections.Counter()
for pc,mn,op in rows:
    if mn=='l32r' and region(pc)=='ROM':
        m=re.search(r'([0-9a-f]{8})',op); lit[region(int(m.group(1),16)) if m else '?']+=1
print('ROM l32r literal regions:',dict(lit))
fn=collections.Counter(sym(pc) for pc,_,_ in rows if region(pc)=='ROM'); tot=sum(fn.values())
print('top ROM functions by instruction count (share of instructions, not time):')
for name,c in fn.most_common(18): print(f'  {c:7d} {100*c/tot:5.1f}% {name}')
print('load/store effective-address classes: NOT AVAILABLE from this trace (a0..a3 only)')
