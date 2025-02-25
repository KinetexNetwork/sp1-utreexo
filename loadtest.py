import threading
import requests
import time

# Configuration
url = "http://127.0.0.1:3000/sp1proof/6"
num_threads = 1
delay_seconds = 0  # Delay between requests in each thread

def worker(thread_id):
    """Worker function that makes continuous requests"""
    while True:
        try:
            response = requests.get(url)
            print(f"Thread-{thread_id}: [{response.status_code}] {response.text.strip()}")
        except requests.exceptions.RequestException as e:
            print(f"Thread-{thread_id}: Error - {str(e)}")
        
        time.sleep(delay_seconds)

# Create and start threads
threads = []
for i in range(num_threads):
    thread = threading.Thread(target=worker, args=(i,), daemon=True)
    thread.start()
    threads.append(thread)

# Keep main thread alive
try:
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    print("\nShutting down threads...")