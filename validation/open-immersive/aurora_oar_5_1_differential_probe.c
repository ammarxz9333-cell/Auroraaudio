#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "oar.h"
#include "test_helpers.h"

#define TEST_SAMPLE_RATE 48000u
#define TEST_FRAMES 256u
#define TEST_FREQUENCY_HZ 440.0f
#define TEST_ELEMENT_ID 1u
#define TEST_CHANNELS 6u

typedef struct {
  const char *name;
  const char *semantic_position;
  float azimuth_degrees;
  float gain_db;
} case_spec_t;

typedef struct {
  double channel_levels[TEST_CHANNELS];
  int finite;
} case_result_t;

static oar_metadata_t *create_gain_metadata(uint32_t gain_id,
                                            float gain_value_db,
                                            uint32_t samples_per_channel) {
  oar_metadata_t *metadata = (oar_metadata_t *)malloc(sizeof(oar_metadata_t));
  if (!metadata) return NULL;
  memset(metadata, 0, sizeof(*metadata));
  metadata->type = ck_metadata_gain;
  metadata->duration = (int)samples_per_channel;
  metadata->gain.id = gain_id;
  metadata->gain.param_type = ck_param_constant;
  metadata->gain.constant_gain = gain_value_db;
  return metadata;
}

static double channel_rms(const float *samples, uint32_t count, int *finite) {
  double sum_squares = 0.0;
  for (uint32_t i = 0; i < count; ++i) {
    double sample = (double)samples[i];
    if (!isfinite(sample)) *finite = 0;
    sum_squares += sample * sample;
  }
  return sqrt(sum_squares / (double)count);
}

static int render_case(const case_spec_t *spec, case_result_t *result) {
  oar_config_t config =
      create_config(ck_oar_layout_51, TEST_FRAMES, TEST_SAMPLE_RATE);
  oar_t *oar = oar_create(&config);
  if (!oar) {
    fprintf(stderr, "oar_create failed for %s\n", spec->name);
    return -1;
  }

  int rc = -1;
  int group_id = oar_add_audio_group(oar);
  if (group_id < 0) {
    fprintf(stderr, "oar_add_audio_group failed for %s\n", spec->name);
    goto cleanup_oar;
  }

  oar_audio_element_config_t element_config = create_object_element_config(1);
  if (oar_add_audio_element(oar, group_id, TEST_ELEMENT_ID, &element_config) !=
      0) {
    fprintf(stderr, "oar_add_audio_element failed for %s\n", spec->name);
    goto cleanup_oar;
  }

  uint32_t samples_per_channel = oar_get_samples_per_channel(oar);
  uint32_t output_channels = oar_get_number_of_output_channels(oar);
  if (samples_per_channel != TEST_FRAMES || output_channels != TEST_CHANNELS) {
    fprintf(stderr,
            "unexpected OAR 5.1 dimensions for %s: frames=%u channels=%u\n",
            spec->name, samples_per_channel, output_channels);
    goto cleanup_oar;
  }

  oar_audio_block_t input;
  if (alloc_audio_block(1, samples_per_channel, &input) != 0) {
    fprintf(stderr, "input allocation failed for %s\n", spec->name);
    goto cleanup_oar;
  }
  generate_sine(input.data, samples_per_channel, TEST_FREQUENCY_HZ,
                (float)TEST_SAMPLE_RATE);
  if (oar_update_audio_element_data(oar, TEST_ELEMENT_ID, &input) != 0) {
    fprintf(stderr, "oar_update_audio_element_data failed for %s\n",
            spec->name);
    free(input.data);
    goto cleanup_oar;
  }
  free(input.data);

  polar_t position = {spec->azimuth_degrees, 0.0f, 1.0f};
  if (set_object_position(oar, TEST_ELEMENT_ID, &position,
                          samples_per_channel) != 0) {
    fprintf(stderr, "position metadata update failed for %s\n", spec->name);
    goto cleanup_oar;
  }

  if (spec->gain_db != 0.0f) {
    oar_metadata_t *gain =
        create_gain_metadata(1, spec->gain_db, samples_per_channel);
    if (!gain) {
      fprintf(stderr, "gain metadata allocation failed for %s\n", spec->name);
      goto cleanup_oar;
    }
    int gain_rc =
        oar_update_audio_element_metadata(oar, TEST_ELEMENT_ID, gain);
    free(gain);
    if (gain_rc != 0) {
      fprintf(stderr, "gain metadata update failed for %s\n", spec->name);
      goto cleanup_oar;
    }
  }

  oar_audio_block_t output;
  if (alloc_audio_block(output_channels, samples_per_channel, &output) != 0) {
    fprintf(stderr, "output allocation failed for %s\n", spec->name);
    goto cleanup_oar;
  }
  if (oar_render(oar, &output) != 0) {
    fprintf(stderr, "oar_render failed for %s\n", spec->name);
    free(output.data);
    goto cleanup_oar;
  }

  result->finite = 1;
  for (uint32_t channel = 0; channel < TEST_CHANNELS; ++channel) {
    result->channel_levels[channel] = channel_rms(
        output.data + channel * samples_per_channel, samples_per_channel,
        &result->finite);
    if (!isfinite(result->channel_levels[channel])) result->finite = 0;
  }
  free(output.data);
  rc = 0;

cleanup_oar:
  oar_destroy(oar);
  return rc;
}

int main(int argc, char **argv) {
  if (argc > 2) {
    fprintf(stderr, "usage: %s [OUTPUT_JSON]\n", argv[0]);
    return 64;
  }

  const case_spec_t specs[] = {
      {"front_left_unity", "front_left", 30.0f, 0.0f},
      {"center_unity", "center", 0.0f, 0.0f},
      {"front_right_unity", "front_right", -30.0f, 0.0f},
      {"surround_left_unity", "surround_left", 110.0f, 0.0f},
      {"surround_right_unity", "surround_right", -110.0f, 0.0f},
      {"center_minus_6db", "center", 0.0f, -6.0f},
  };
  const size_t case_count = sizeof(specs) / sizeof(specs[0]);
  case_result_t results[sizeof(specs) / sizeof(specs[0])];

  for (size_t i = 0; i < case_count; ++i) {
    if (render_case(&specs[i], &results[i]) != 0) return 1;
  }

  FILE *out = stdout;
  if (argc == 2) {
    out = fopen(argv[1], "w");
    if (!out) {
      perror("fopen");
      return 1;
    }
  }

  fprintf(out,
          "{\n"
          "  \"schema_version\": 1,\n"
          "  \"implementation\": \"aomedia-oar-1.0.0\",\n"
          "  \"layout\": \"5.1\",\n"
          "  \"sample_rate\": %u,\n"
          "  \"frames_per_case\": %u,\n"
          "  \"channel_order\": [\"FL\", \"FR\", \"FC\", \"LFE\", \"SL\", \"SR\"],\n"
          "  \"coordinate_convention\": \"positive-azimuth-left\",\n"
          "  \"lfe_semantics\": \"non-directional-zero-for-object-render\",\n"
          "  \"cases\": [\n",
          TEST_SAMPLE_RATE, TEST_FRAMES);

  for (size_t i = 0; i < case_count; ++i) {
    fprintf(out,
            "    {\"name\": \"%s\", \"semantic_position\": \"%s\", "
            "\"azimuth_degrees\": %.6f, \"gain_db\": %.6f, "
            "\"frame_count\": %u, \"channel_levels\": [%.12g, %.12g, %.12g, %.12g, %.12g, %.12g], "
            "\"finite\": %s}%s\n",
            specs[i].name, specs[i].semantic_position, specs[i].azimuth_degrees,
            specs[i].gain_db, TEST_FRAMES, results[i].channel_levels[0],
            results[i].channel_levels[1], results[i].channel_levels[2],
            results[i].channel_levels[3], results[i].channel_levels[4],
            results[i].channel_levels[5], results[i].finite ? "true" : "false",
            i + 1 == case_count ? "" : ",");
  }
  fprintf(out, "  ]\n}\n");

  if (out != stdout && fclose(out) != 0) {
    perror("fclose");
    return 1;
  }
  fprintf(stderr, "AURORA-OAR-5-1-DIFFERENTIAL-PROBE-PASS\n");
  return 0;
}
