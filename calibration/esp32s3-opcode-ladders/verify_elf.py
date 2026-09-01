#!/usr/bin/env python3
"""Verify every opcode-ladder body against its IDF 6.1 byte contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

from elftools.elf.elffile import ELFFile


class VerificationError(ValueError):
    pass


EXPECTED_BODIES: dict[str, tuple[int, str]] = {
    "opcode_beqz_taken": (1024, "077053f92b23e6639994bc6629639a974cfe95324aa05ecbd875475db69192ea"),
    "opcode_beqz_not_taken": (2048, "bf6c75920ab6fce1168a1df5a3d4c5aa7526451f0e56ca56c62ca8dcd38091c9"),
    "opcode_bnez_taken": (1024, "f46ba1329a1a5054079a270678435a1125d4769cb21732a02e697f41d7d6232a"),
    "opcode_bnez_not_taken": (2048, "19b2d5e25af317e3ed579cd24e75cda7ad58fe4b47b54217992976ca26eb3255"),
    "opcode_bltz_taken": (1024, "81a05a84dcbf8083b70b8d394950343817083e06f3b6c6a1ffd001c3442929df"),
    "opcode_bltz_not_taken": (2048, "ca4a4828e8c4aedd5de9a6f4b30d8a59eb5859db37ead77ecc8f32e93078ac52"),
    "opcode_bgez_taken": (1024, "085706dc0a5d5b604144cc836e95f340aae306f7dbf8ab71350da4e5792b354f"),
    "opcode_bgez_not_taken": (2048, "54b2233c0c525514929b247d3e79e0b381bf115e82995ee86d54610f3cc78c47"),
    "opcode_beqi_taken": (1024, "f83226c00e932a88b7d72048497e1c835797a96a3e899d1d9da83d6994f32773"),
    "opcode_beqi_not_taken": (2048, "1b384deaf1e2200f6a010c3eac847d54a30c1087feffa4d40475fe0c86b939cd"),
    "opcode_bnei_taken": (1024, "493bde25c6ad1e315fe5ebb62bef3e6f72d2a130c8c31532e4b06d43d1a65167"),
    "opcode_bnei_not_taken": (2048, "a785794eb4401e4749399ca186aaff57bdcecf128880172969064eacec8dc2ab"),
    "opcode_beq_taken": (1024, "99e8dfa3ec32c378cc5eb49c94248394078944899f00afb2045ef7001053ace3"),
    "opcode_beq_not_taken": (2048, "0af8e5aa79217633528ffef49ecb23867e18bba5d1831c6903fcc44ac6a57f7c"),
    "opcode_bne_taken": (1024, "8db5426560fa6b46688db477a9b15defb5f7611b57868407c87063cde6aeafa7"),
    "opcode_bne_not_taken": (2048, "3e34552d3e2561cca67f1620f3160b58e38e6fcdd4ac8a82f240f0b7a7fed175"),
    "opcode_blt_taken": (1024, "d1fc93ddfd8aabd1debbfd1364e99b40f2257ff67bcf38c421a4568b8257a865"),
    "opcode_blt_not_taken": (2048, "2ad2ad10bca54ac45034a02f7ebb26bf717a377902b7d1fa04761ed13d4a99f8"),
    "opcode_bge_taken": (1024, "a5b7936e706c778830f25ec39885e231362c983d0f143c6e63c95b611b081db5"),
    "opcode_bge_not_taken": (2048, "595a07588933f6acdc687b085f20f804e22c8edee1510cb44a1826104875920b"),
    "opcode_bltu_taken": (1024, "00c17dc4065d2f93037ea9b07d7df26d88820dd640c7ffcab5491d057627a93c"),
    "opcode_bltu_not_taken": (2048, "d937d81a10e8b5842cbf3c923bdaef803dab59eed832f5c19847369929f66d53"),
    "opcode_bgeu_taken": (1024, "bd71e7d03c42e1f8da6bec7314ea4dbbb0c7ad4e7279d7bd07ff9e868d16c76e"),
    "opcode_bgeu_not_taken": (2048, "113b40e06122381d6da9a63177b0f1be83d0ec2bbe648ef0c3fe04b1ad41f729"),
    "opcode_blti_taken": (1024, "5638fd318976103303d0c6fe44964f2b2f09ca847c1c1283009a26f94f9b1c6c"),
    "opcode_blti_not_taken": (2048, "5ea083db839555f533ae53546b9304129528abfc3e1f3116e0509312b9088b76"),
    "opcode_bgei_taken": (1024, "e9223e50158aaa7c93822461f8ecd436139bec93a750e39fa4c250a079e0807b"),
    "opcode_bgei_not_taken": (2048, "cd40ad456726945eac98f56d9e76031483f68743722703068cc580f3f0be982d"),
    "opcode_bltui_taken": (1024, "5d53acdb132465502a4f12d5f6a168fd5fc1d020817fa8ecf2313fd969e1bb5c"),
    "opcode_bltui_not_taken": (2048, "6b0fe36128794ae82b0a29c5bb77eaec6ab164a0caf119d5bbf20180c17f0fc8"),
    "opcode_bgeui_taken": (1024, "c92dc2499dab6b1e15356b34487de60c157374a1a21f71e36bddb1852ee4a040"),
    "opcode_bgeui_not_taken": (2048, "b591386a51bf3af4f4be202a24cbf539b73331ae7f2ea3269898df414c433881"),
    "opcode_bany_taken": (1024, "40da4f0fabb76c4b5d3738fd109d75e8e553f57608ad5e11e460e72e5e78698b"),
    "opcode_bany_not_taken": (2048, "6da44dac2f54e9d2fd4c6d833687d2fa2987e4394e425c210b8f201f4f966e47"),
    "opcode_bnone_taken": (1024, "afcf794d5f92eedb9952b365dfdea508312389a8ad7e43bcc2900f677d51e2f2"),
    "opcode_bnone_not_taken": (2048, "5e0e5d5ec0291d3d35536cef84759fc7fba6c6ed709be9510226253e8a40299c"),
    "opcode_ball_taken": (1024, "d8eccef25dd6c15a7dd5d78d955930b2c797eeb7c511ffc7ec7e37669545f285"),
    "opcode_ball_not_taken": (2048, "f1a1dfa5c8fc8b0edd223c5a921b19144919034f6f9ccab827788ebd977b164c"),
    "opcode_bnall_taken": (1024, "52f8094c5ada79094ea46399e58bd954983436ed623815232dfc11dc4b82fd52"),
    "opcode_bnall_not_taken": (2048, "b9fd7b2e985f81eccb5dfbacff3534728e44185171e73661e7d4264d5db50446"),
    "opcode_bbc_taken": (1024, "da061ef0421cdb699d431f0651b7723d83db038e65f0d5acc7209c58585cec5a"),
    "opcode_bbc_not_taken": (2048, "87389d04cb82f232d382eab2be290fba50f865ff092f945e502133357cb1980c"),
    "opcode_bbs_taken": (1024, "75faaa16aa30dcc58f4270a1b005a8e6163c9c0968af7573d05ef9b7235e3e59"),
    "opcode_bbs_not_taken": (2048, "292ecc636eb4f832eaf6552cb267500d89c7eb8325956ce3b7c4ebd44734eb81"),
    "opcode_bbci_taken": (1024, "87692000239ba2a3ecbac65b53ee78df5a0023e035b99eceac4b5bc2d949d4fc"),
    "opcode_bbci_not_taken": (2048, "d7eb0cd256af50001867380b04ed982a88c1f8d6c19a7866288fcd204d5b1090"),
    "opcode_bbsi_taken": (1024, "f3c4dcde28e877f2f343be03bf03799d6e18d35542011e4d0a72554c7d747d4b"),
    "opcode_bbsi_not_taken": (2048, "45478cad30b65c3654f18488a9e920e71c484670f2c51a2b5c092288df388ad9"),
    "opcode_beqz_n_taken": (1024, "33d2f4c7cd1e08311359f9c0d9e5c8198f21d200bc4779660a19f38570e6ed77"),
    "opcode_beqz_n_not_taken": (2048, "ab58a549b83d7a388e5a60a8068dc92b1acc9ed4afa75c686912b98d4ff3e18c"),
    "opcode_bnez_n_taken": (1024, "e022b8fdb808bce1238792a120e766431ce4efc5c761f30c9299560c469c408f"),
    "opcode_bnez_n_not_taken": (2048, "91e76950382f4f4a41edb5eb735664f433163f9a3405b40bf93e78a43434b76a"),
    "opcode_j": (1024, "fd51a5a37005c2b6b381d853c972c56fc8f4faf0cfd84b442cf3223ccb67c70f"),
    "opcode_jx": (2048, "a0efb27c98cda4689d3bab3706014affc3de4e6a1d22f809aa0edfd4569a451b"),
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
    "opcode_l32r": (768, "c2f9c2b8e3db71107830502c9fef7f61993fc434a5abb796114ee2a192191a00"),
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
            unit = 8 if function.endswith("_not_taken") else 4
            targets = [start + unit * (index + 1) for index in range(256)]
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
    args = parser.parse_args()
    if args.result.exists():
        print(f"refusing to overwrite result: {args.result}", file=sys.stderr)
        return 2
    try:
        result = verify(args.elf, args.manifest)
        args.result.parent.mkdir(parents=True, exist_ok=True)
        args.result.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    except (OSError, KeyError, TypeError, json.JSONDecodeError, VerificationError) as error:
        print(f"ELF verification failed: {error}", file=sys.stderr)
        return 2
    print(f"ELF verification passed: {args.result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
