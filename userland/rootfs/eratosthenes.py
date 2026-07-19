limit = 100
is_prime = [True] * (limit + 1)
is_prime[0] = False
is_prime[1] = False

candidate = 2
while candidate * candidate <= limit:
    if is_prime[candidate]:
        multiple = candidate * candidate
        while multiple <= limit:
            is_prime[multiple] = False
            multiple += candidate
    candidate += 1

primes = [number for number in range(1, limit + 1) if is_prime[number]]
print("eratosthenes:", primes)
