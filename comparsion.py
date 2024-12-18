import os
import json
import matplotlib.pyplot as plt

# Define the directories
dir1 = "circuit/metrics-cycles-new"
dir2 = "circuit/metrics-cycles-new-2"

def extract_data(directory):
    """Extract block_size and total_instructions from JSON files in the directory."""
    data_points = []
    
    for filename in os.listdir(directory):
        if filename.endswith(".json"):
            filepath = os.path.join(directory, filename)
            with open(filepath, 'r') as file:
                data = json.load(file)
                # Collect block_size and total_instructions as a tuple
                data_points.append((data['block_size'], data['total_instructions']))
    
    # Sort data points by block_size
    data_points.sort(key=lambda x: x[0])
    block_sizes, total_instructions = zip(*data_points)
    return list(block_sizes), list(total_instructions)

# Extract data from both directories
block_sizes1, total_instructions1 = extract_data(dir1)
block_sizes2, total_instructions2 = extract_data(dir2)

# Plot the data
plt.figure(figsize=(10, 6))
plt.plot(block_sizes1, total_instructions1, label="Directory 1: metrics-cycles-new", marker='o')
plt.plot(block_sizes2, total_instructions2, label="Directory 2: metrics-cycles-new-2", marker='o')

# Add labels, legend, and title
plt.xlabel("Block size")
plt.ylabel("Total Instructions")
plt.title("Comparison of Total Instructions by Block size")
plt.legend()
plt.grid(True)

# Show the plot
plt.show()
