import os
import sqlite3
import time


def main():
    """Keeps a WAL-mode connection open while the drill writes and recovers a marker."""
    connection = sqlite3.connect(
        f"file:{os.environ['DATABASE_PATH']}?mode=rw", uri=True, timeout=5
    )
    connection.execute("PRAGMA journal_mode=WAL")
    connection.execute("PRAGMA synchronous=FULL")
    connection.execute("CREATE TABLE IF NOT EXISTS checks (token TEXT NOT NULL)")
    connection.commit()
    while True:
        time.sleep(3600)


main()
