#include <math.h>
#include <stdio.h>
#include <stdlib.h>

#include "oar.h"
#include "test_helpers.h"

#define PROBE_SAMPLES 256
#define PROBE_RATE 48000
#define PROBE_ELEMENT_ID 1

static const float kAzimuths[] = {-60.0f, -30.0f, -15.0f, 0.0f,
                                  15.0f,  30.0f,  60.0f};

static double channel_energy(const float *samples, uint32_t count) {
  double energy = 0.0;
  for (uint32_t i = 0; i < count; ++i) {
    const double sample = samples[i];
    energy += sample * sample;
  }
  return energy;
}

static int render_case(float azimuth, double *left_share, double *right_share) {
  oar_config_t config =
      create_config(ck_oar_layout_stereo, PROBE_SAMPLES, PROBE_RATE);
  oar_t *oar = oar_create(&config);
  if (!oar) return -1;

  int result = -1;
  int gid = oar_add_audio_group(oar);
  if (gid < 0) goto cleanup_oar;

  oar_audio_element_config_t element = create_object_element_config(1);
  if (oar_add_audio_element(oar, (uint32_t)gid, PROBE_ELEMENT_ID, &element) !=
      ck_oar_ok)
    goto cleanup_oar;

  oar_audio_block_t input;
  if (alloc_audio_block(1, PROBE_SAMPLES, &input) != 0) goto cleanup_oar;
  generate_sine(input.data, PROBE_SAMPLES, 997.0f, (float)PROBE_RATE);
  if (oar_update_audio_element_data(oar, PROBE_ELEMENT_ID, &input) !=
      ck_oar_ok) {
    free(input.data);
    goto cleanup_oar;
  }
  free(input.data);

  polar_t position = {azimuth, 0.0f, 1.0f};
  if (set_object_position(oar, PROBE_ELEMENT_ID, &position, PROBE_SAMPLES) !=
      ck_oar_ok)
    goto cleanup_oar;

  const uint32_t channels = oar_get_number_of_output_channels(oar);
  if (channels != 2) goto cleanup_oar;

  oar_audio_block_t output;
  if (alloc_audio_block(channels, PROBE_SAMPLES, &output) != 0)
    goto cleanup_oar;
  if (oar_render(oar, &output) != ck_oar_ok) {
    free(output.data);
    goto cleanup_oar;
  }

  const double left = channel_energy(output.data, PROBE_SAMPLES);
  const double right =
      channel_energy(output.data + PROBE_SAMPLES, PROBE_SAMPLES);
  free(output.data);

  const double total = left + right;
  if (!isfinite(left) || !isfinite(right) || !isfinite(total) || total <= 0.0)
    goto cleanup_oar;

  *left_share = left / total;
  *right_share = right / total;
  result = 0;

cleanup_oar:
  oar_destroy(oar);
  return result;
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "usage: %s OUTPUT_TSV\n", argv[0]);
    return 2;
  }

  FILE *output = fopen(argv[1], "w");
  if (!output) {
    perror("fopen");
    return 3;
  }

  fprintf(output, "OAR_DIFF_V1\n");
  fprintf(output, "azimuth_degrees\tleft_power_share\tright_power_share\n");

  const size_t case_count = sizeof(kAzimuths) / sizeof(kAzimuths[0]);
  for (size_t i = 0; i < case_count; ++i) {
    double left_share = 0.0;
    double right_share = 0.0;
    if (render_case(kAzimuths[i], &left_share, &right_share) != 0) {
      fprintf(stderr, "OAR differential render failed at azimuth %.1f\n",
              kAzimuths[i]);
      fclose(output);
      return 4;
    }
    fprintf(output, "%.1f\t%.9f\t%.9f\n", kAzimuths[i], left_share,
            right_share);
  }

  if (fclose(output) != 0) {
    perror("fclose");
    return 5;
  }

  printf("OAR-DIFFERENTIAL-PROBE-PASS cases=%zu output=%s\n", case_count,
         argv[1]);
  return 0;
}
