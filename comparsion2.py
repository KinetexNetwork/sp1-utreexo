import os
import json

# Define the directories
dir1 = "circuit/metrics-cycles-new"
dir2 = "circuit/metrics-cycles-new-2"

def extract_data(directory):
    """Extract data as a dictionary with block_height as the key and total_instructions as the value."""
    data = {}
    for filename in os.listdir(directory):
        if filename.endswith(".json"):
            filepath = os.path.join(directory, filename)
            with open(filepath, 'r') as file:
                json_data = json.load(file)
                data[json_data['block_height']] = json_data['total_instructions']
    return data

# Extract data from both directories
data1 = extract_data(dir1)
data2 = extract_data(dir2)

# Find common block heights
common_heights = set(data1.keys()).intersection(data2.keys())

# Calculate and print the percentage difference for each common block height
for height in sorted(common_heights):
    instructions1 = data1[height]
    instructions2 = data2[height]
    if instructions2 != 0:  # Prevent division by zero
        percentage_diff = ((instructions2 - instructions1) / instructions2) * 100
        print(f"Block Height: {height}, Instructions Difference: {percentage_diff:.2f}% less in metrics-cycles-new")
    else:
        print(f"Block Height: {height}, Instructions Difference: Division by zero (metrics-cycles-new-2 has 0 instructions)")
