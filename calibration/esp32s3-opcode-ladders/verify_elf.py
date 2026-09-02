#!/usr/bin/env python3
"""Verify every opcode-ladder body against its IDF 6.1 byte contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

from elftools.elf.elffile import ELFFile


class VerificationError(ValueError):
    pass


FUNCTION_HEADER_RE = re.compile(r"^([0-9a-f]+) <([^>]+)>:$")
DROM_REFERENCE_RE = re.compile(r"\((3[cd][0-9a-f]{6})(?:\s|<)")
INSTRUCTION_RE = re.compile(
    r"^([0-9a-f]+):\s+[0-9a-f]+\s+([a-zA-Z0-9_.]+)(?:\s+(.*))?$"
)


EXPECTED_BODIES: dict[str, tuple[int, str]] = {
    "opcode_beqz_taken": (1024, "077053f92b23e6639994bc6629639a974cfe95324aa05ecbd875475db69192ea"),
    "opcode_beqz_not_taken": (768, "c97b0f3e4e016a4d74bc72dc0119c3055d2acb2b5d201c8e004871b15a0a746e"),
    "opcode_bnez_taken": (1024, "f46ba1329a1a5054079a270678435a1125d4769cb21732a02e697f41d7d6232a"),
    "opcode_bnez_not_taken": (768, "f1202fd402e50afe391d8df87d238dcff558f480f2fe258e952bf7501168f16b"),
    "opcode_bltz_taken": (1024, "81a05a84dcbf8083b70b8d394950343817083e06f3b6c6a1ffd001c3442929df"),
    "opcode_bltz_not_taken": (768, "ea89941e21ccb0a6a03d3425e3e96e6773e7dea25315a0a141f56213afb234ae"),
    "opcode_bgez_taken": (1024, "085706dc0a5d5b604144cc836e95f340aae306f7dbf8ab71350da4e5792b354f"),
    "opcode_bgez_not_taken": (768, "cd4fab4bfc17b7cac1476bca3e48b48f885d2283fe2b6578cfe64c8e42267827"),
    "opcode_beqi_taken": (1024, "f83226c00e932a88b7d72048497e1c835797a96a3e899d1d9da83d6994f32773"),
    "opcode_beqi_not_taken": (768, "d06e2afa4c3a546a9eb5c3f61bc3f9466a6f784aa54b3617e6ebbeb1f4eace12"),
    "opcode_bnei_taken": (1024, "493bde25c6ad1e315fe5ebb62bef3e6f72d2a130c8c31532e4b06d43d1a65167"),
    "opcode_bnei_not_taken": (768, "c638d8455ee468f775105ada849eee62fb54b07a032e8731985e1a9bd3533188"),
    "opcode_beq_taken": (1024, "99e8dfa3ec32c378cc5eb49c94248394078944899f00afb2045ef7001053ace3"),
    "opcode_beq_not_taken": (768, "d46e2b7fb4e61da5ed6ce3d37e0baf81a01fe39accc5fac2a96da2b29645ced6"),
    "opcode_bne_taken": (1024, "8db5426560fa6b46688db477a9b15defb5f7611b57868407c87063cde6aeafa7"),
    "opcode_bne_not_taken": (768, "1bb01c80f005bb95df5c5e243fc279cbfe363063ebbd2c915daf27397b1631b6"),
    "opcode_blt_taken": (1024, "d1fc93ddfd8aabd1debbfd1364e99b40f2257ff67bcf38c421a4568b8257a865"),
    "opcode_blt_not_taken": (768, "41807661ecccbb6ed189bc18c885e0b547846231c1ea8bcefc7327afe9df5365"),
    "opcode_bge_taken": (1024, "a5b7936e706c778830f25ec39885e231362c983d0f143c6e63c95b611b081db5"),
    "opcode_bge_not_taken": (768, "c4334a559c65822ee0dbd58defa11ced9559e588d1d9d80435e1819e7bd3b3e9"),
    "opcode_bltu_taken": (1024, "00c17dc4065d2f93037ea9b07d7df26d88820dd640c7ffcab5491d057627a93c"),
    "opcode_bltu_not_taken": (768, "bc62705c89d59b5b273c7a66473c110a31ac6b7087cfc761ef0533d146d8b257"),
    "opcode_bgeu_taken": (1024, "bd71e7d03c42e1f8da6bec7314ea4dbbb0c7ad4e7279d7bd07ff9e868d16c76e"),
    "opcode_bgeu_not_taken": (768, "0deb7e838b41372ab1a3d1a2fef58043a4547bffddcd2d70324ccd50474aac8e"),
    "opcode_blti_taken": (1024, "5638fd318976103303d0c6fe44964f2b2f09ca847c1c1283009a26f94f9b1c6c"),
    "opcode_blti_not_taken": (768, "07625a1f4b523cd33e6c3acc21885811ed070b724740de7f2dde3a41b584d87e"),
    "opcode_bgei_taken": (1024, "e9223e50158aaa7c93822461f8ecd436139bec93a750e39fa4c250a079e0807b"),
    "opcode_bgei_not_taken": (768, "0b5d165887161def1e2d3fd20d963eea17cf09362bb63cf3c3969859a76d315b"),
    "opcode_bltui_taken": (1024, "5d53acdb132465502a4f12d5f6a168fd5fc1d020817fa8ecf2313fd969e1bb5c"),
    "opcode_bltui_not_taken": (768, "a5ee77334bacf520fc3c4d9d7a4e48c8ec2ed08225efaf2efec08d7ec268825f"),
    "opcode_bgeui_taken": (1024, "c92dc2499dab6b1e15356b34487de60c157374a1a21f71e36bddb1852ee4a040"),
    "opcode_bgeui_not_taken": (768, "fcd1424ea7758ac1c0c8e21748609257c76b416657c7c4b4f121037b8ee2f3a9"),
    "opcode_bany_taken": (1024, "40da4f0fabb76c4b5d3738fd109d75e8e553f57608ad5e11e460e72e5e78698b"),
    "opcode_bany_not_taken": (768, "1944f75b0ac084d3c74ffa131c1caf29b1d1a52f655e07557fe3673f2a363e64"),
    "opcode_bnone_taken": (1024, "afcf794d5f92eedb9952b365dfdea508312389a8ad7e43bcc2900f677d51e2f2"),
    "opcode_bnone_not_taken": (768, "181e6f6964ece3ca74b286c2cd5fa316e5ede9f4afab027efa90c5a15e707a59"),
    "opcode_ball_taken": (1024, "d8eccef25dd6c15a7dd5d78d955930b2c797eeb7c511ffc7ec7e37669545f285"),
    "opcode_ball_not_taken": (768, "8852af0bec3af4df9ff5b652e5d74f5f37f5236e114af6e189a25c9f471dd7fd"),
    "opcode_bnall_taken": (1024, "52f8094c5ada79094ea46399e58bd954983436ed623815232dfc11dc4b82fd52"),
    "opcode_bnall_not_taken": (768, "e6f2dc4e27484f5968a9655c177818620294e21b90f93cb511fb501f609693ef"),
    "opcode_bbc_taken": (1024, "da061ef0421cdb699d431f0651b7723d83db038e65f0d5acc7209c58585cec5a"),
    "opcode_bbc_not_taken": (768, "01cc41f5cda7af0180f6bd7ede236af9eb15188670511a86aa1c7da31cf13c5b"),
    "opcode_bbs_taken": (1024, "75faaa16aa30dcc58f4270a1b005a8e6163c9c0968af7573d05ef9b7235e3e59"),
    "opcode_bbs_not_taken": (768, "677c63d39f2f09a827fee5d0ca2992fffe41a0ea55686e2c3d3d16ad6cdeb4ea"),
    "opcode_bbci_taken": (1024, "87692000239ba2a3ecbac65b53ee78df5a0023e035b99eceac4b5bc2d949d4fc"),
    "opcode_bbci_not_taken": (768, "ede9fb60af5660fa97c27497c2fc00aebfa9b56dbb8119fb9ca07f100c7e6488"),
    "opcode_bbsi_taken": (1024, "f3c4dcde28e877f2f343be03bf03799d6e18d35542011e4d0a72554c7d747d4b"),
    "opcode_bbsi_not_taken": (768, "b79153f3656bd66c47e722e2e95ac88acd9d915ebf544e0c140652774f6258a0"),
    "opcode_beqz_n_taken": (1024, "33d2f4c7cd1e08311359f9c0d9e5c8198f21d200bc4779660a19f38570e6ed77"),
    "opcode_beqz_n_not_taken": (512, "4482c507186f60ea35880e835e0a84119249349dfef427b353d88dad8d8eb1de"),
    "opcode_bnez_n_taken": (1024, "e022b8fdb808bce1238792a120e766431ce4efc5c761f30c9299560c469c408f"),
    "opcode_bnez_n_not_taken": (512, "2dd9beef827ec31550b72e8255f47253fd5cf58244194537e809fce63c7ce13d"),
    "opcode_j": (1024, "fd51a5a37005c2b6b381d853c972c56fc8f4faf0cfd84b442cf3223ccb67c70f"),
    "opcode_jx": (2048, "8d87f8c892e28dbc3b98e8e8f9f52d46d32ba1a67d4cf8e57dd6049721449bbb"),
    "opcode_call0_ret": (768, "0b8afdbfb563c628540bac4135e845cccf9af11d3548c56837be1d19b9efdfb1"),
    "opcode_callx0_ret": (768, "71d4adc47420d201834fa01558405726a8c1f7e4b78f2efe34f85bc4cb89df6c"),
    "opcode_call8_retw": (768, "b6acffb473d589557a2c292374e78688ce66e4dbc6592af17dfa5feddd4bb911"),
    "opcode_callx8_retw": (768, "ff6efba9b924b860236756167b6f03fe86d00b617e7a6b380258057b8de3eca5"),
    "opcode_loop": (2048, "bef62c77322769fc46e53903d1870c7964d42c2423efb39d1cde9e40068c0dcb"),
    "opcode_loopnez": (2048, "3afe48ebbb45f8be538b908324fbfb9aa0b3a0ef7c4612618ec21bbdc174b848"),
    "opcode_loopgtz": (2048, "2a3ed2cbef8a1ab7671ee07450ca19a10ee49deeaa3256e6eedffb49062d779a"),
    "opcode_issue_nop_baseline": (512, "00808678219dfefeea6f2959d107fc349756d83ed293ca4fd2457c686a57b319"),
    "opcode_mull": (768, "506418c802efe33a5e7e6c9eec1fdae6e49170ea227ef463782bc595dd83db8d"),
    "opcode_mulsh": (768, "f9e5a0a7ba261a246da5bce05800ca86b8efce8d77373db425d8ffd57df3fa57"),
    "opcode_muluh": (768, "a3d48ee6eea49b30e4d936919b9df4f10c887a49cd68c9e6fb3032832d63ee28"),
    "opcode_quos": (768, "08e00b0c8fe757721b16ff49d904b137bc02067776b6873b26a37fb82f1e9b14"),
    "opcode_quou": (768, "304f2008bc04753b446434e6e4b83742ca7410e1b7058dddf363f1b8028dc85b"),
    "opcode_rems": (768, "c0b6b9f04a9cbc558cc2b8ee40883b74c618e7f8587e85c26fc41634bf372039"),
    "opcode_remu": (768, "bd7823b825abf0c3a25ca503f4266df716549730e7840643d740d74830f0eb3d"),
    "opcode_nsa": (768, "d4a5fade7c66b501e0815a9315142b0d275581bb72ab92ab23728cdcc6155f29"),
    "opcode_nsau": (768, "72ae3d81b79817a7839c9ca2ed5b9a85b96da7446f16f1cf245cccd8f7b2d5b9"),
    "opcode_sext": (768, "0e78d8cdbbaaee0120d382d1bceb57e2781ba8a339907c4978caf42472710a13"),
    "opcode_l32r": (768, "7cbdcde9cff761f67a648ebb7c07149de0b0fcd0fbdc74816b9930fff5c09730"),
    "opcode_s32c1i": (768, "14a22b2b1bcd8b96116544d514b73f2215e7841e3e626f9d92625209fecf51c7"),
    "opcode_memw": (768, "f2a8417650cabf0b312eda34b48970d9f882d075807ae8a9d07bd30d8d752060"),
    "opcode_extw": (768, "dec41899a803f0330d7686d6c5e45eff13813d8951ffa4dfc1d4feb96d5d9c84"),
    "opcode_rsr": (768, "7a21d64ce2cb6b5c36d6ba91f4802ebc0d79d0826b814850d8b9ad882dd07094"),
    "opcode_wsr": (768, "917682c924e1dff0dd7acb81783e69afe0bbffbfb3504dbfd14a9edb17351f75"),
    "opcode_xsr": (768, "2ef32d7a0f9c5c67b88f2b9f75324d4dd25f1b180b689c8b1ccc6f5f67ecc623"),
    "opcode_rsync": (768, "b3993de6c5d28d4dd9457cbc25437ce882ddbe58b0cc376bfb47f4edaa1c0fa1"),
    "opcode_isync": (768, "11a472f1e4b7bbeade508d58c54b05248c25ed60b995d5f817d7861d946980ee"),
    "opcode_movsp": (768, "d5feca7faf6c0105362125fb50750cf787e7333c1ce5a9fc0efa088fe4ff9a0c"),
    "opcode_min": (768, "53e333162c905556dab40cbc61fe3a3cb0846f803ee5c64969e97f77d64056a3"),
    "opcode_max": (768, "134231c5c8de0a566a57df22bfa7387e5fb10698364175cc1750a1cd6786f2fb"),
    "opcode_minu": (768, "bb6aa050bf7323e99ffaefaf7853b34b56c15dd44875336989a516bbb62d1901"),
    "opcode_maxu": (768, "210a241dd0e7fbbebdc2824ebf06b0584e8fece15de67e85c2d575e9b07c2c40"),
    "load_use_distance_1": (1536, "e164db66ff56dde0d392fa8252fff9d1d4e83b240443466947976435bb63a7b2"),
    "load_use_distance_2": (2304, "bbddb5edbcae133b15853c5d4395ce51c6c3d4665571c565621a00e87497ef0a"),
}


def function_for_cell(cell: str) -> str:
    if cell.startswith("load_use_distance_"):
        return cell
    return f"opcode_{cell}"


def load_symbols(elf: ELFFile) -> dict[str, int]:
    table = elf.get_section_by_name(".symtab")
    if table is None:
        raise VerificationError("ELF has no symbol table")
    return {symbol.name: int(symbol.entry.st_value) for symbol in table.iter_symbols()}


def read_virtual(elf: ELFFile, address: int, length: int) -> bytes:
    for segment in elf.iter_segments():
        low = int(segment.header.p_vaddr)
        high = low + int(segment.header.p_filesz)
        if low <= address and address + length <= high:
            offset = address - low
            return segment.data()[offset : offset + length]
    raise VerificationError(f"body at {address:#x} is not in a loadable ELF segment")


def verify_measurement_window(disassembly: str) -> dict[str, object]:
    address = None
    instructions: list[str] = []
    in_window = False
    for raw_line in disassembly.splitlines():
        line = raw_line.strip()
        header = FUNCTION_HEADER_RE.match(line)
        if header is not None:
            if in_window:
                break
            if header.group(2) == "measure_probe_samples":
                address = int(header.group(1), 16)
                in_window = True
            continue
        if in_window and re.match(r"^[0-9a-f]+:", line):
            instructions.append(line)

    if address is None or not instructions:
        raise VerificationError("missing measure_probe_samples disassembly")
    if not 0x40370000 <= address < 0x403E0000:
        raise VerificationError("measure_probe_samples is outside internal IRAM")
    drom_references = [line for line in instructions if DROM_REFERENCE_RE.search(line)]
    descriptor_references = [line for line in instructions if "<probes>" in line]
    if drom_references or descriptor_references:
        raise VerificationError("measurement window loads from the flash descriptor table")
    if not any("callx8" in line for line in instructions):
        raise VerificationError("measurement window has no indirect probe call")
    if sum("rsr.ccount" in line for line in instructions) < 2:
        raise VerificationError("measurement window has fewer than two CCOUNT reads")
    parsed = []
    for line in instructions:
        match = INSTRUCTION_RE.match(line)
        if match is None:
            raise VerificationError("cannot parse measurement-window instruction")
        parsed.append(
            (int(match.group(1), 16), match.group(2), match.group(3) or "")
        )

    max_attempts = [
        index
        for index, (_, mnemonic, operands) in enumerate(parsed)
        if mnemonic.startswith("movi") and operands.endswith(", -200")
    ]
    sample_quotas = [
        index
        for index, (_, mnemonic, operands) in enumerate(parsed)
        if mnemonic == "addi" and operands.endswith(", -100")
    ]
    if len(max_attempts) != 1 or len(sample_quotas) != 1:
        raise VerificationError("measurement window lacks the 100-of-200 retry bounds")
    max_index = max_attempts[0]
    max_register = parsed[max_index][2].split(",", 1)[0]
    if max_index + 1 >= len(parsed):
        raise VerificationError("200-attempt bound does not control the retry loop")
    _, max_add_mnemonic, max_add_operands = parsed[max_index + 1]
    max_add_registers = [item.strip() for item in max_add_operands.split(",")]
    if (
        not max_add_mnemonic.startswith("add")
        or len(max_add_registers) != 3
        or max_add_registers[0] != max_register
        or max_add_registers[2] != max_register
    ):
        raise VerificationError("200-attempt bound does not control the retry loop")

    accepted_path = None
    for index in range(len(parsed) - 1):
        _, mismatch_mnemonic, mismatch_operands = parsed[index]
        _, zero_mnemonic, zero_operands = parsed[index + 1]
        if not mismatch_mnemonic.startswith("bnez") or not zero_mnemonic.startswith(
            "beqz"
        ):
            continue
        mismatch_target = re.search(r",\s*([0-9a-f]+)\b", mismatch_operands)
        zero_target = re.search(r",\s*([0-9a-f]+)\b", zero_operands)
        if (
            mismatch_target is not None
            and zero_target is not None
            and mismatch_target.group(1) == zero_target.group(1)
        ):
            accepted_path = (index, int(mismatch_target.group(1), 16))
            break
    if accepted_path is None:
        raise VerificationError("dirty and zero-cycle samples do not share a skip path")

    rejection_index, rejection_target = accepted_path
    mismatch_register = parsed[rejection_index][2].split(",", 1)[0]
    zero_register = parsed[rejection_index + 1][2].split(",", 1)[0]
    if not any(
        mnemonic == "or" and operands.startswith(f"{mismatch_register},")
        for _, mnemonic, operands in parsed[:rejection_index]
    ):
        raise VerificationError("cache-counter result does not gate sample acceptance")
    if not any(
        mnemonic == "sub" and operands.startswith(f"{zero_register},")
        for _, mnemonic, operands in parsed[:rejection_index]
    ):
        raise VerificationError("elapsed cycle count does not gate sample acceptance")
    target_index = next(
        (
            index
            for index, (address, _, _) in enumerate(parsed)
            if address == rejection_target
        ),
        None,
    )
    if target_index is None or target_index <= rejection_index + 1:
        raise VerificationError("sample rejection does not skip the accepted-sample path")
    acceptance = parsed[rejection_index + 2 : target_index]
    if not any(mnemonic.startswith("s32i") for _, mnemonic, _ in acceptance):
        raise VerificationError("accepted-sample path does not store a sample")
    if not any(
        mnemonic.startswith("addi") and operands.endswith(", 1")
        for _, mnemonic, operands in acceptance
    ):
        raise VerificationError("accepted-sample path does not advance the clean quota")

    quota_index = sample_quotas[0]
    quota_register = parsed[quota_index][2].split(",", 1)[0]
    max_exit = any(
        mnemonic.startswith("beqz") and operands.startswith(f"{max_register},")
        for _, mnemonic, operands in parsed[quota_index + 1 :]
    )
    quota_loop = any(
        mnemonic.startswith("bnez")
        and operands.startswith(f"{quota_register},")
        and int(re.search(r",\s*([0-9a-f]+)\b", operands).group(1), 16) < inst_address
        for inst_address, mnemonic, operands in parsed[quota_index + 1 :]
        if re.search(r",\s*([0-9a-f]+)\b", operands) is not None
    )
    if quota_index != target_index or not max_exit or not quota_loop:
        raise VerificationError("100-of-200 bounds do not control the retry loop")

    ccount_reads = [
        index
        for index, (_, mnemonic, _) in enumerate(parsed[:rejection_index])
        if mnemonic == "rsr.ccount"
    ]
    counter_window = parsed[ccount_reads[-1] + 1 : rejection_index]
    if sum(mnemonic.startswith("l32i") for _, mnemonic, _ in counter_window) < 5:
        raise VerificationError("cache counters are not all read before sample acceptance")
    if sum(mnemonic == "or" for _, mnemonic, _ in counter_window) < 4:
        raise VerificationError("cache counters are not folded before sample acceptance")
    return {
        "symbol": "measure_probe_samples",
        "address": address,
        "instructions": len(instructions),
        "dromDescriptorLoads": 0,
        "acceptedSamplesRequired": 100,
        "maxAttempts": 200,
        "dirtySamplesDiscarded": True,
    }


def verify(path: Path, manifest_path: Path) -> dict[str, object]:
    manifest = json.loads(manifest_path.read_text())
    cells = [cell["id"] for cell in manifest["cells"]]
    functions = [function_for_cell(cell) for cell in cells]
    if len(cells) != 88 or len(cells) != len(set(cells)):
        raise VerificationError("manifest must contain exactly 88 unique cells")
    if set(functions) != set(EXPECTED_BODIES):
        missing = sorted(set(functions) - set(EXPECTED_BODIES))
        extra = sorted(set(EXPECTED_BODIES) - set(functions))
        raise VerificationError(f"body contract mismatch, missing={missing}, extra={extra}")

    results = []
    with path.open("rb") as stream:
        elf = ELFFile(stream)
        symbols = load_symbols(elf)
        for function in functions:
            start_name = f"{function}_body_start"
            end_name = f"{function}_body_end"
            if start_name not in symbols or end_name not in symbols:
                raise VerificationError(f"missing body symbols for {function}")
            start = symbols[start_name]
            end = symbols[end_name]
            expected_length, expected_sha = EXPECTED_BODIES[function]
            if not function.startswith("opcode_loop") and start % 4 != 0:
                raise VerificationError(f"{function} body is not 4-byte aligned")
            if not 0x40370000 <= start < end <= 0x403E0000:
                raise VerificationError(f"{function} body is outside internal IRAM")
            if end - start != expected_length:
                raise VerificationError(
                    f"{function} body length is {end - start}, expected {expected_length}"
                )
            body = read_virtual(elf, start, end - start)
            actual_sha = hashlib.sha256(body).hexdigest()
            if actual_sha != expected_sha:
                raise VerificationError(
                    f"{function} encoding SHA-256 is {actual_sha}, expected {expected_sha}"
                )
            results.append(
                {
                    "symbol": function,
                    "address": start,
                    "bytes": len(body),
                    "encodingSha256": actual_sha,
                }
            )

        branch_functions = functions[:52]
        for function in branch_functions:
            start = symbols[f"{function}_body_start"]
            if function.endswith("_not_taken") and "_n_" in function:
                targets = [start + 20 + 16 * (index // 8) for index in range(256)]
            elif function.endswith("_not_taken"):
                targets = [start + 12 * (index // 4 + 1) for index in range(256)]
            else:
                targets = [start + 4 * (index + 1) for index in range(256)]
            if any(target % 4 != 0 for target in targets):
                raise VerificationError(f"{function} has an unaligned branch target")

        for function in ("opcode_loop", "opcode_loopnez", "opcode_loopgtz"):
            start = symbols[f"{function}_body_start"]
            body_starts = [start + 8 * index + 3 for index in range(256)]
            if any(address % 4 != 0 for address in body_starts):
                raise VerificationError(f"{function} has an unaligned loop body")

    return {
        "ok": True,
        "cells": len(cells),
        "branchTargetsAligned": 52 * 256,
        "loopBodiesAligned": 3 * 256,
        "bodies": results,
        "elf": str(path),
        "elfSha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("elf", type=Path)
    parser.add_argument("result", type=Path)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path(__file__).with_name("probe-cells.json"),
    )
    parser.add_argument("--objdump", default="xtensa-esp32s3-elf-objdump")
    args = parser.parse_args()
    if args.result.exists():
        print(f"refusing to overwrite result: {args.result}", file=sys.stderr)
        return 2
    try:
        result = verify(args.elf, args.manifest)
        disassembly = subprocess.run(
            [args.objdump, "-d", str(args.elf)],
            check=True,
            text=True,
            capture_output=True,
        ).stdout
        result["measurementWindow"] = verify_measurement_window(disassembly)
        args.result.parent.mkdir(parents=True, exist_ok=True)
        args.result.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    except (
        OSError,
        KeyError,
        TypeError,
        json.JSONDecodeError,
        subprocess.CalledProcessError,
        VerificationError,
    ) as error:
        print(f"ELF verification failed: {error}", file=sys.stderr)
        return 2
    print(f"ELF verification passed: {args.result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
