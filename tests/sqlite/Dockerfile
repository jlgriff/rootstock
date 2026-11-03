FROM python:3.14-alpine
COPY app.py /app.py
CMD ["python", "/app.py"]
HEALTHCHECK --interval=1s --timeout=2s --retries=20 CMD python -c "import os,sqlite3; sqlite3.connect('file:' + os.environ['DATABASE_PATH'] + '?mode=rw', uri=True).execute('SELECT count(*) FROM checks').fetchone()"
