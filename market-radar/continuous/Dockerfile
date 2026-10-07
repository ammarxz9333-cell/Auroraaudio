FROM python:3.13-slim
WORKDIR /app
COPY continuous/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt && useradd --uid 10001 --create-home radar
COPY . /app
RUN mkdir -p /app/runtime && chown -R radar:radar /app/runtime
USER radar
ENV PYTHONUNBUFFERED=1 RADAR_BIND=0.0.0.0
CMD ["python", "-m", "continuous.service"]
