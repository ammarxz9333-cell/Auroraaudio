#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "oar.h"
#include "oar_base.h"
#include "oar_metadata.h"

#define SAMPLE_RATE 48000u
#define SAMPLES_PER_CHANNEL 256u
#define PI_F 3.14159265358979323846f

static double rms(const float *samples, uint32_t count) {
  double sum = 0.0;
  for (uint32_t i = 0; i < count; ++i) {
    const double value = samples[i];
    sum += value * value;
  }
  return sqrt(sum / (double)count);
}

static int render_probe(float azimuth, float gain_db, double *left_rms,
                        double *right_rms) {
  oar_config_t config = {
      .target_layout = ck_oar_layout_stereo,
      .samples_per_channel = SAMPLES_PER_CHANNEL,
      .sampling_rate = SAMPLE_RATE,
  };
  oar_t *oar = oar_create(&config);
  if (!oar) return -1;

  int result = -1;
  const int gid = oar_add_audio_group(oar);
  if (gid < 0) goto done;

  oar_audio_element_config_t element;
  memset(&element, 0, sizeof(element));
  element.type = ck_object_based;
  element.obc.num_objects = 1;
  if (oar_add_audio_element(oar, (uint32_t)gid, 1, &element) != ck_oar_ok)
    goto done;

  float input[SAMPLES_PER_CHANNEL];
  for (uint32_t i = 0; i < SAMPLES_PER_CHANNEL; ++i) {
    input[i] = 0.25f * sinf(2.0f * PI_F * 440.0f * (float)i /
                           (float)SAMPLE_RATE);
  }
  oar_audio_block_t input_block = {
      .data = input,
      .channels = 1,
      .samples_per_channel = SAMPLES_PER_CHANNEL,
  };
  if (oar_update_audio_element_data(oar, 1, &input_block) != ck_oar_ok)
    goto done;

  oar_metadata_t position;
  memset(&position, 0, sizeof(position));
  position.type = ck_metadata_object_positions;
  position.duration = (int)SAMPLES_PER_CHANNEL;
  position.object_positions.param_type = ck_param_constant;
  position.object_positions.position_type = ck_polar;
  position.object_positions.num_objects = 1;
  position.object_positions.polar_positions[0].azimuth = azimuth;
  position.object_positions.polar_positions[0].elevation = 0.0f;
  position.object_positions.polar_positions[0].distance = 1.0f;
  if (oar_update_audio_element_metadata(oar, 1, &position) != ck_oar_ok)
    goto done;

  if (gain_db != 0.0f) {
    oar_metadata_t gain;
    memset(&gain, 0, sizeof(gain));
    gain.type = ck_metadata_gain;
    gain.duration = (int)SAMPLES_PER_CHANNEL;
    gain.gain.id = 100;
    gain.gain.param_type = ck_param_constant;
    gain.gain.constant_gain = gain_db;
    if (oar_update_audio_element_metadata(oar, 1, &gain) != ck_oar_ok)
      goto done;
  }

  float output[2u * SAMPLES_PER_CHANNEL];
  memset(output, 0, sizeof(output));
  oar_audio_block_t output_block = {
      .data = output,
      .channels = 2,
      .samples_per_channel = SAMPLES_PER_CHANNEL,
  };
  if (oar_render(oar, &output_block) != ck_oar_ok) goto done;

  *left_rms = rms(output, SAMPLES_PER_CHANNEL);
  *right_rms = rms(output + SAMPLES_PER_CHANNEL, SAMPLES_PER_CHANNEL);
  if (!isfinite(*left_rms) || !isfinite(*right_rms)) goto done;
  result = 0;

done:
  oar_destroy(oar);
  return result;
}

int main(void) {
  const float probes[][2] = {
      {30.0f, 0.0f},
      {0.0f, 0.0f},
      {-30.0f, 0.0f},
      {0.0f, -6.0f},
  };
  puts("OAR-DIFF-PROBE schema=1 layout=stereo samples_per_channel=256 sample_rate=48000");
  for (size_t i = 0; i < sizeof(probes) / sizeof(probes[0]); ++i) {
    double left = 0.0;
    double right = 0.0;
    if (render_probe(probes[i][0], probes[i][1], &left, &right) != 0) {
      fprintf(stderr, "OAR-PROBE-ERROR azimuth=%.3f gain_db=%.3f\n",
              probes[i][0], probes[i][1]);
      return 1;
    }
    printf("OAR-PROBE azimuth=%.3f gain_db=%.3f left=%.9f right=%.9f\n",
           probes[i][0], probes[i][1], left, right);
  }
  return 0;
}
