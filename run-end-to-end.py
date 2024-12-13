# This python script extracts data fetched by previously runned server, runs it through
# the input generator, then through the circuit.
from os import system
from subprocess import run, PIPE
import logging
import json
import os
from enum import Enum

RED = "\033[91m"
GREEN = "\033[92m"
YELLOW = "\033[93m"
BLUE = "\033[94m"
DEFAULT_COLOR = "\033[0m"

IMPROVEMENTS = list()
HEIGHTS = list()
CYCLES = list()
BASELINE_CYCLES = list()

class Result(Enum):
    SUCCESS = 0
    FAILURE = 1

def run_one(txnum: int) -> Result:
    if move_data_to_input(txnum) != Result.SUCCESS:
        return Result.FAILURE
    if run_input_generator(txnum) != Result.SUCCESS:
        return Result.FAILURE
    if move_data_to_circuit(txnum) != Result.SUCCESS:
        return Result.FAILURE
    if run_circuit(txnum) != Result.SUCCESS:
        return Result.FAILURE
    print_report(txnum)
    HEIGHTS.append(get_height(txnum))
    return Result.SUCCESS


def get_height(txnum: int) -> int:
    report_path = f"circuit/metrics-cycles-new/{txnum}.json"
    with open(report_path, "r") as report_file:
        report_json = json.load(report_file)
        height = report_json["block_height"]
        return height


def move_data_to_input(txnum: int) -> Result:
    server_data_path = f"server/acc-datas/block-{txnum}txs/"
    errno = system(f"mkdir -p input-generator/acc-data/block-{txnum}txs/")
    if errno != 0:
        logging.error(f"Failed to create directory input-generator/acc-data/block-{txnum}txs/")
        return Result.FAILURE
    input_data_path = f"input-generator/acc-data/block-{txnum}txs/"
    errno = system(f"cp -r {server_data_path} {input_data_path}")
    if errno != 0:
        logging.error(f"Failed to move data from {server_data_path} to {input_data_path}")
        return Result.FAILURE
    logging.info(f"Moved data from {server_data_path} to {input_data_path}")
    return Result.SUCCESS


def run_input_generator(txnum: int) -> Result:
    command = f"cargo r -- --exact {txnum}"
    result = run(command, shell=True, cwd="input-generator", stdout=PIPE, stderr=PIPE)
    if result.returncode != 0:
        with open(f"input-generator-error-{txnum}.log", "wb") as f:
            f.write(result.stdout)
            f.write(result.stderr)
            logging.error(f"Input generator failed for txnum = {txnum}. Dumped error log to input-generator-error-{txnum}.log")
            return Result.FAILURE
    logging.info(f"Input generator succeeded for txnum = {txnum}")
    return Result.SUCCESS


def move_data_to_circuit(txnum: int) -> Result:
    # Move data from input to circuit
    input_data_path = f"input-generator/processed-acc-data/*"
    circuit_data_path = f"circuit/acc-data/"
    errno = system(f"cp -r {input_data_path} {circuit_data_path}")
    if errno != 0:
        logging.error(f"Failed to move data from {input_data_path} to {circuit_data_path}")
        return Result.FAILURE
    logging.info(f"Moved data from {input_data_path} to {circuit_data_path}")
    return Result.SUCCESS


def run_circuit(txnum: int) -> Result:
    command = f"cargo r --release -- --execute --exact {txnum}"
    result = run(command, shell=True, cwd="circuit/script", stdout=PIPE, stderr=PIPE)
    if result.returncode != 0:
        with open(f"circuit-error-{txnum}.log", "wb") as f:
            f.write(result.stdout)
            f.write(result.stderr)
            logging.error(f"Circuit failed for txnum = {txnum}. Dumped error log to circuit-error-{txnum}.log")
            return Result.FAILURE
    logging.info(f"Circuit succeeded for txnum = {txnum}")
    return Result.SUCCESS


def print_report(txnum: int) -> None:
    report_path = f"circuit/metrics-cycles-new/{txnum}.json"
    baseline_path = f"circuit/metrics-cycles/{txnum}.json"
    with open(report_path, "r") as report_file:
        with open(baseline_path, "r") as baseline_file:
            report_json = json.load(report_file)
            cycles = report_json["total_instructions"]
            baseline_json = json.load(baseline_file)
            baseline_cycles = baseline_json["total_instructions"]
            improvement = baseline_cycles / cycles
            improvement_color = RED if improvement < 1 else YELLOW if improvement < 10 else GREEN if improvement < 100 else BLUE
            print(f"TxNum = {txnum}; Cycles = {cycles}; Baseline Cycles = {baseline_cycles}; {improvement_color} Improvement = {improvement} times {DEFAULT_COLOR}")
            IMPROVEMENTS.append(improvement)
            avg_improvement = sum(IMPROVEMENTS) / len(IMPROVEMENTS)
            avg_improvement_color = RED if avg_improvement < 1 else YELLOW if avg_improvement < 10 else GREEN if avg_improvement < 100 else BLUE
            print(f"{avg_improvement_color}Average improvement atm = {avg_improvement} {DEFAULT_COLOR}")
            CYCLES.append(cycles)
            BASELINE_CYCLES.append(baseline_cycles)



def get_available_txnums() -> list[int]:
    server_data_path = "server/acc-datas/"
    txnums = []
    for entry in os.listdir(server_data_path):
        if os.path.isdir(os.path.join(server_data_path, entry)):
            try:
                txnum = int(entry.split('-')[1].replace('txs', ''))
                txnums.append(txnum)
            except (IndexError, ValueError):
                continue
    return sorted(txnums)


def cleanup() -> None:
    system("rm -rf input-generator/acc-data/*")
    system("rm -rf circuit/acc-data/*")
    system("rm -rf circuit/metrics-cycles-new/*")


def print_plot() -> None:
    import matplotlib.pyplot as plt

    heights = HEIGHTS
    cycles = CYCLES
    baseline_cycles = BASELINE_CYCLES

    plt.figure(figsize=(10, 6))
    plt.plot(heights, cycles, label='Cycles', marker='o')
    plt.plot(heights, baseline_cycles, label='Baseline Cycles', marker='x')
    plt.xlabel('Block Height')
    plt.ylabel('Cycles')
    plt.title('Cycles and Baseline Cycles vs Block Height')
    plt.legend()
    plt.grid(True)
    plt.show()


def main() -> None:
    logging.basicConfig(level=logging.ERROR)
    for txnum in get_available_txnums():
        run_one(txnum)
    print_plot()
    cleanup()



if __name__ == "__main__":
    main()
