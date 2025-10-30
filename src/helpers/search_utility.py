"""
Search utility for fuzzy subsequence matching.
Matches items where search letters appear in order.
"""


def subsequence_match(search_string, target_string):
    """
    Check if search_string is a subsequence of target_string.
    Returns (is_match, score) tuple.

    Score is based on:
    - Lower is better (distance between matched characters)
    - Matches at word boundaries get bonus (lower score)
    """
    if not search_string:
        return True, 0

    # Remove spaces from search string for more flexible matching
    search_lower = search_string.lower().replace(" ", "")
    target_lower = target_string.lower()

    search_idx = 0
    target_idx = 0
    match_positions = []

    # Find all positions where search characters match
    while search_idx < len(search_lower) and target_idx < len(target_lower):
        if search_lower[search_idx] == target_lower[target_idx]:
            match_positions.append(target_idx)
            search_idx += 1
        target_idx += 1

    # If we didn't match all search characters, it's not a match
    if search_idx < len(search_lower):
        return False, float('inf')

    # Calculate score based on spread and word boundaries
    score = 0

    # Add penalty for spread (distance between first and last match)
    if len(match_positions) > 1:
        spread = match_positions[-1] - match_positions[0]
        score += spread

    # Give bonus (negative score) for matches at word boundaries
    for pos in match_positions:
        # Beginning of string
        if pos == 0:
            score -= 10
        # After space or punctuation
        elif pos > 0 and target_lower[pos - 1] in ' -_./\\':
            score -= 5

    # Give bonus for matches near the beginning
    if match_positions:
        score += match_positions[0] * 0.5

    return True, score


def fuzzy_search(search_string, items, key_func):
    """
    Search items using subsequence matching.

    Args:
        search_string: The search query
        items: List of items to search
        key_func: Function to extract searchable text from each item

    Returns:
        List of matching items, sorted by relevance (best first)
    """
    if not search_string:
        return items

    matches = []

    for item in items:
        target_text = key_func(item)
        is_match, score = subsequence_match(search_string, target_text)

        if is_match:
            matches.append((score, item))

    # Sort by score (lower is better)
    matches.sort(key=lambda x: x[0])

    return [item for score, item in matches]


def check_exact_shortcut_match(search_string, items, shortcut_key='shortcut'):
    """
    Check if search_string exactly matches any item's shortcut.
    Returns the matching item or None.

    Args:
        search_string: The search query
        items: List of items (dicts with shortcut field)
        shortcut_key: The key name for the shortcut field

    Returns:
        The matching item if found, None otherwise
    """
    search_lower = search_string.lower()

    for item in items:
        if item.get(shortcut_key, '').lower() == search_lower:
            return item

    return None
