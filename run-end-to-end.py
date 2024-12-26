# This python script extracts data fetched by previously runned server, runs it through
# the input generator, then through the circuit.
from os import system
from subprocess import run, PIPE
import logging
import json
import os
from enum import Enum
from math import log
import argparse

IS_TEST = False

def parse_arguments():
    global IS_TEST
    parser = argparse.ArgumentParser(description="Add --test flag to the script.")
    parser.add_argument('--test', action='store_true', help='Set this flag to enable test mode.')
    args = parser.parse_args()
    
    if args.test:
        IS_TEST = True

RED = "\033[91m"
GREEN = "\033[92m"
YELLOW = "\033[93m"
BLUE = "\033[94m"
DEFAULT_COLOR = "\033[0m"

IMPROVEMENTS = list()
HEIGHTS = list()
CYCLES = list()
BASELINE_CYCLES = list()


INITIAL_DATA_PATH = "server/acc-datas/"

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
    report_path = f"circuit/metrics/{txnum}.json"
    with open(report_path, "r") as report_file:
        report_json = json.load(report_file)
        height = report_json["block_height"]
        return height


def move_data_to_input(txnum: int) -> Result:
    server_data_path = f"{INITIAL_DATA_PATH}block-{txnum}txs/"
    errno = system(f"mkdir -p input-generator/acc-data/block-{txnum}txs/")
    if errno != 0:
        logging.error(f"Failed to create directory input-generator/acc-data/block-{txnum}txs/")
        return Result.FAILURE
    input_data_path = f"input-generator/acc-data/block-{txnum}txs/"
    errno = system(f"cp -r {server_data_path}* {input_data_path}")
    if errno != 0:
        logging.error(f"Failed to move data from {server_data_path} to {input_data_path}")
        return Result.FAILURE
    logging.info(f"Moved data from {server_data_path} to {input_data_path}")
    return Result.SUCCESS


def run_input_generator(txnum: int) -> Result:
    command = f"cargo r -- --exact {txnum}"
    result = run(command, shell=True, cwd="input-generator", stdout=PIPE, stderr=PIPE)
    if result.returncode != 0:
        if not IS_TEST:
            with open(f"input-generator-error-{txnum}.log", "wb") as f:
                f.write(result.stdout)
                f.write(result.stderr)
                logging.error(f"Input generator failed for txnum = {txnum}. Dumped error log to input-generator-error-{txnum}.log")
                return Result.FAILURE
        else:
            print(f"Input generator failed for txnum = {txnum}")
            print(f"Output: {result.stdout}")
            print(f"Error: {result.stderr}")
            raise Exception("Input generator failed")
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
        if not IS_TEST:
            with open(f"circuit-error-{txnum}.log", "wb") as f:
                f.write(result.stdout)
                f.write(result.stderr)
                logging.error(f"Circuit failed for txnum = {txnum}. Dumped error log to circuit-error-{txnum}.log")
                return Result.FAILURE
        else:
            print(f"Circuit failed for txnum = {txnum}")
            print(f"Output: {result.stdout}")
            print(f"Error: {result.stderr}")
            raise Exception("Circuit failed")
            
    logging.info(f"Circuit succeeded for txnum = {txnum}")
    return Result.SUCCESS


def print_report(txnum: int) -> None:
    try:
        report_path = f"circuit/metrics/{txnum}.json"
        with open(report_path, "r") as report_file:
            report_json = json.load(report_file)
            cycles = report_json["total_instructions"]
            acc_size = report_json["acc_size"]
            height = report_json["block_height"]
            block_size = report_json["block_size"]
            max_cpu_speed = 25 * 10**6 # 25 MHz
            min_cpu_speed = 10 * 10**6 # 10 MHz
            min_time = cycles / max_cpu_speed
            max_time = cycles / min_cpu_speed
            color = RED if max_time > 5 * 60 else YELLOW if max_time > 3 * 60 else GREEN
            report = f"TxNum = {txnum}; Height = {height}; Time: {color}{min_time:.2f}s-{max_time:.2f}s{DEFAULT_COLOR}"

            acc_size_before_processing = os.path.getsize(f"input-generator/acc-data/block-{txnum}txs/acc-beffore.txt")
            acc_size_after_processing = os.path.getsize(f"input-generator/processed-acc-data/block-{txnum}txs/acc-before.txt")

            # We assume that our algorithm is O(block_size * log(acc_size)), thus, while our assumption is
            # correct, this coeffecient should be have relatively low variance.
            coeffecient = (block_size * log(acc_size, 2)) / cycles
            report += f"Formula coeffecient: {coeffecient:.2f}"
            report +=f"Acc before processing: {sizeof_fmt(acc_size_before_processing)}; Acc after processing: {sizeof_fmt(acc_size_after_processing)}"
            print(report)


    except Exception as e:
        print(f"Failed to write report with error: {e}")
        if IS_TEST:
            raise e


def sizeof_fmt(num, suffix="B"):
    """Convert bytes to a human-readable format."""
    for unit in ["", "K", "M", "G", "T", "P", "E", "Z"]:
        if abs(num) < 1024.0:
            return f"{num:3.1f}{unit}{suffix}"
        num /= 1024.0
    return f"{num:.1f}Y{suffix}"


def get_available_txnums() -> list[int]:
    server_data_path = INITIAL_DATA_PATH
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
    system("rm -rf circuit/metrics/*")



def main() -> None:
    logging.basicConfig(level=logging.ERROR)
    parse_arguments()
    if IS_TEST:
        global INITIAL_DATA_PATH
        INITIAL_DATA_PATH = "test-data/"
    print(f"{YELLOW}Warn: all times was NOT measured, but estimated{DEFAULT_COLOR}")
    for txnum in get_available_txnums():
        res = run_one(txnum)
        if IS_TEST and res == Result.FAILURE:
            raise Exception("Failed to run one of the txnums")
    cleanup()


from concurrent.futures import ThreadPoolExecutor, as_completed

def process_txnum(txnum):
    res = run_one(txnum)
    if IS_TEST and res == Result.FAILURE:
        raise Exception(f"Failed to run txnum: {txnum}")
    return res


def concurrent_main():
    logging.basicConfig(level=logging.ERROR)
    parse_arguments()
    if IS_TEST:
        global INITIAL_DATA_PATH
        INITIAL_DATA_PATH = "test-data/"
    print(f"{YELLOW}Warn: all times was NOT measured, but estimated{DEFAULT_COLOR}")
    with ThreadPoolExecutor() as executor:
        futures = {executor.submit(process_txnum, txnum): txnum for txnum in get_available_txnums()}
        for future in as_completed(futures):
            try:
                result = future.result()
            except Exception as e:
                print(f"Error: {e}")
    cleanup()


if __name__ == "__main__":
    concurrent_main()
