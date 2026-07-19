import math
import struct


angular_frequency = 2.0
time_step = 0.01
step_count = 100


def acceleration(position):
    return -(angular_frequency * angular_frequency) * position


position = 1.0
velocity = 0.0
time = 0.0

for _ in range(step_count):
    k1_position = velocity
    k1_velocity = acceleration(position)

    k2_position = velocity + 0.5 * time_step * k1_velocity
    k2_velocity = acceleration(position + 0.5 * time_step * k1_position)

    k3_position = velocity + 0.5 * time_step * k2_velocity
    k3_velocity = acceleration(position + 0.5 * time_step * k2_position)

    k4_position = velocity + time_step * k3_velocity
    k4_velocity = acceleration(position + time_step * k3_position)

    position += time_step * (
        k1_position + 2.0 * k2_position + 2.0 * k3_position + k4_position
    ) / 6.0
    velocity += time_step * (
        k1_velocity + 2.0 * k2_velocity + 2.0 * k3_velocity + k4_velocity
    ) / 6.0
    time += time_step

phase = angular_frequency * time
expected_position = math.cos(phase)
expected_velocity = -angular_frequency * math.sin(phase)
state_error = math.sqrt(
    (position - expected_position) ** 2 + (velocity - expected_velocity) ** 2
)

initial_energy = 0.5 * angular_frequency * angular_frequency
energy = 0.5 * velocity * velocity + (
    0.5 * angular_frequency * angular_frequency * position * position
)
energy_error = math.fabs(energy - initial_energy)

assert state_error < 1e-7
assert energy_error < 1e-8


def double_words(value):
    return struct.unpack(">II", struct.pack(">d", value))


position_words = double_words(position)
velocity_words = double_words(velocity)
state_error_words = double_words(state_error)
energy_error_words = double_words(energy_error)

print(
    "rk4-state-ieee754:",
    [
        position_words[0],
        position_words[1],
        velocity_words[0],
        velocity_words[1],
        state_error_words[0],
        state_error_words[1],
        energy_error_words[0],
        energy_error_words[1],
    ],
)
