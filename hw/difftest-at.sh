#!/bin/sh
# Run both hardware and emulator to a breakpoint, then single-step N instructions and compare.
#   hw/difftest-at.sh <pc-hex> [steps]
set -e
cd "$(dirname "$0")/.."
PC=${1:?pc}; N=${2:-3000}
OCD=$(ls -d ~/.espressif/tools/openocd-esp32/*/openocd-esp32 | head -1); SCR=$OCD/share/openocd/scripts
GDB=$(ls ~/.espressif/tools/xtensa-esp-elf-gdb/*/xtensa-esp-elf-gdb/bin/xtensa-esp32s3-elf-gdb | head -1)
pkill -f openocd-esp32 2>/dev/null || true; sleep 1
STRAP=$(grep "^0x60004038" hw/jtag-regs.txt | awk '{print $2}')
$OCD/bin/openocd -s $SCR -f board/esp32s3-builtin.cfg -c "esp32s3.cpu0 configure -rtos none" > hw/openocd.log 2>&1 & OPID=$!
sleep 4
$GDB -batch -nx -ex "set pagination off" -ex "set confirm off" -ex "target extended-remote :3333" -ex "monitor reset halt" -ex "flushregs" \
     -ex "hbreak *0x$PC" -ex "continue" -ex "flushregs" -ex "info registers pc" -ex "delete" \
     -ex "set \$steps=$N" -ex 'set $outfile="hw/hw-trace.txt"' -ex "source hw/steptrace.py" > hw/gdb.log 2>&1 || true
kill $OPID 2>/dev/null || true
./target/release/esp32sim --boot rom --flash-mb 16 --flash-image hw/flash-0-1M.bin --efuse-regs hw/efuse.txt --regs-init hw/reset-regs.txt \
     --strap 0x$STRAP --regtrace hw/emu-trace.txt --regtrace-from-pc 0x$PC --regtrace-max $N --console none --no-dump 2>/dev/null
python3 hw/compare.py hw/hw-trace.txt hw/emu-trace.txt
