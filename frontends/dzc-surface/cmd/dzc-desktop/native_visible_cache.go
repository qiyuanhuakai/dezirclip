package main

// Only the visible rows start requests. The cache keeps scrolling from
// retaining every preview visited, and limits simultaneous image decoding.
type nativeVisibleCache[K comparable, V any] struct {
	values  map[K]V
	pending map[K]bool
	recent  []K
}

func (c *nativeVisibleCache[K, V]) lookup(key K) (V, bool) {
	value, ok := c.values[key]
	return value, ok
}

func (c *nativeVisibleCache[K, V]) begin(key K) bool {
	if len(c.pending) >= 3 || c.pending[key] {
		return false
	}
	if c.pending == nil {
		c.pending = make(map[K]bool)
	}
	c.pending[key] = true
	return true
}

func (c *nativeVisibleCache[K, V]) finish(key K, value V) {
	delete(c.pending, key)
	if c.values == nil {
		c.values = make(map[K]V)
	}
	if len(c.recent) == 96 {
		delete(c.values, c.recent[0])
		copy(c.recent, c.recent[1:])
		c.recent = c.recent[:95]
	}
	c.values[key] = value
	c.recent = append(c.recent, key)
}
